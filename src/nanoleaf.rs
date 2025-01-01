use crate::constants;
use crate::utils;
use palette::{hsv::Hsv, rgb::Srgb, FromColor};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs::{self, OpenOptions};
use std::io::prelude::*;
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::path::Path;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Axis {
    X,
    #[default]
    Y,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Sort {
    #[default]
    Asc,
    Desc,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Panel {
    pub id: u16,
    x: i16,
    y: i16,
}

#[derive(Clone, Debug)]
pub struct Effect {
    pub name: String,
    pub colors: Vec<Srgb<u8>>,
}

#[derive(Debug)]
pub struct NanoleafDevice {
    pub ip: Ipv4Addr,
    pub name: String,
    pub curr_effect: Option<String>,
    pub panels: Vec<Panel>,
    token: String,
}

impl NanoleafDevice {
    /// Create a new Nanoleaf device handle. If a device with this IP isn't present in the device file,
    /// request its token add it there
    pub fn new(ip: &Ipv4Addr, nl_device_file: &Path) -> Result<Self, anyhow::Error> {
        let ip = ip.to_owned();
        let token = match Self::find_token(&ip, nl_device_file)? {
            Some(token) => token,
            None => {
                let token = Self::get_new_token(&ip)?;
                Self::add_device_entry(&ip, &token, nl_device_file)?;
                token
            }
        };
        let name = Self::get_name(&ip, &token)?;
        let curr_effect = Self::get_curr_effect(&ip, &token)?;
        let panels = Self::get_panels(&ip, &token)?;

        Ok(NanoleafDevice {
            ip,
            name,
            curr_effect,
            panels,
            token,
        })
    }

    fn find_token(ip: &Ipv4Addr, nl_device_file: &Path) -> Result<Option<String>, anyhow::Error> {
        if !Path::exists(nl_device_file) {
            return Ok(None);
        }
        let nl_devices = fs::read_to_string(nl_device_file)?;
        for device in nl_devices.lines() {
            let split = device.split(';').collect::<Vec<_>>();
            if split.len() != 2 {
                return Err(anyhow::Error::msg(
                    "Invalid nl_devices file, every line should look like {IP};{TOKEN}",
                ));
            }
            if split[0] == ip.to_string() {
                return Ok(Some(split[1].trim_end().to_string()));
            }
        }
        Ok(None)
    }

    fn get_new_token(ip: &Ipv4Addr) -> Result<String, anyhow::Error> {
        let Ok(res) = utils::request_post(
            &format!("http://{}:{}/api/v1/new", ip, constants::NL_API_PORT),
            None,
        ) else {
            return Err(anyhow::Error::msg(format!("Couldn't connect to the Nanoleaf device at {}, make sure that the control lights are flashing while you're trying to connect.", ip)));
        };
        let res_json: serde_json::Value = serde_json::from_str(&res)?;
        Ok(res_json["auth_token"]
            .as_str()
            .unwrap()
            .trim_end()
            .to_string())
    }

    fn add_device_entry(
        ip: &Ipv4Addr,
        token: &str,
        nl_device_file: &Path,
    ) -> Result<(), anyhow::Error> {
        let nl_device_dir = match nl_device_file.parent() {
            Some(parent) => parent,
            None => {
                return Err(anyhow::Error::msg(format!(
                    "Path '{}' is invalid",
                    nl_device_file.to_string_lossy()
                )));
            }
        };
        if !Path::try_exists(nl_device_dir)? {
            fs::create_dir(nl_device_dir)?;
        }
        let mut nl_device_file_handle = OpenOptions::new()
            .create(true)
            .append(true)
            .open(nl_device_file)?;
        nl_device_file_handle.write_all(format!("{};{}\n", ip, token).as_bytes())?;

        Ok(())
    }

    fn get_name(ip: &Ipv4Addr, token: &str) -> Result<String, anyhow::Error> {
        let Ok(res) = utils::request_get(&format!(
            "http://{}:{}/api/v1/{}",
            ip,
            constants::NL_API_PORT,
            token
        )) else {
            return Err(anyhow::Error::msg(format!(
                "Couldn't reach the Nanoleaf device at {}.",
                ip
            )));
        };
        let res_json: serde_json::Value = serde_json::from_str(&res)?;

        Ok(String::from(res_json["name"].as_str().unwrap()))
    }

    fn get_curr_effect(ip: &Ipv4Addr, token: &str) -> Result<Option<String>, anyhow::Error> {
        let Ok(res) = utils::request_get(&format!(
            "http://{}:{}/api/v1/{}/effects/select",
            ip,
            constants::NL_API_PORT,
            token
        )) else {
            return Err(anyhow::Error::msg(format!(
                "Couldn't reach the Nanoleaf device at {}.",
                ip
            )));
        };
        let res_text: String = serde_json::from_str(&res)?;
        if res_text == "*Solid*"
            || res_text == "*Dynamic*"
            || res_text == "*Static*"
            || res_text == "*ExtControl*"
        {
            Ok(None)
        } else {
            Ok(Some(res_text))
        }
    }

    fn get_panels(ip: &Ipv4Addr, token: &str) -> Result<Vec<Panel>, anyhow::Error> {
        let Ok(res) = utils::request_get(&format!(
            "http://{}:{}/api/v1/{}/panelLayout/layout",
            ip,
            constants::NL_API_PORT,
            token
        )) else {
            return Err(anyhow::Error::msg(format!(
                "Couldn't reach the Nanoleaf device at {}.",
                ip
            )));
        };
        let res_json: serde_json::Value = serde_json::from_str(&res)?;
        let res_panels = res_json["positionData"].as_array().unwrap();
        let mut panels = Vec::new();
        for panel in res_panels.iter() {
            let id = panel["panelId"].as_u64().unwrap() as u16;
            let (x, y) = (
                panel["x"].as_i64().unwrap() as i16,
                panel["y"].as_i64().unwrap() as i16,
            );
            panels.push(Panel { id, x, y });
        }

        Ok(panels)
    }

    pub fn get_udp_socket(&self, port: u16) -> Result<UdpSocket, anyhow::Error> {
        let socket_addr = SocketAddrV4::new(Ipv4Addr::new(0, 0, 0, 0), port);
        let socket = UdpSocket::bind(socket_addr)?;
        let nl_addr = SocketAddrV4::new(self.ip, constants::NL_UDP_PORT);
        socket.connect(nl_addr)?;

        Ok(socket)
    }

    pub fn get_effect_list(&self) -> Result<Vec<Effect>, anyhow::Error> {
        let Ok(res) = utils::request_get(&format!(
            "http://{}:{}/api/v1/{}/effects/effectsList",
            self.ip,
            constants::NL_API_PORT,
            self.token
        )) else {
            return Err(anyhow::Error::msg(format!(
                "Couldn't reach the Nanoleaf device at {}.",
                self.ip
            )));
        };
        let res_list: Vec<String> = serde_json::from_str(&res)?;
        let mut palettes = Vec::with_capacity(res_list.len());
        for effect_name in res_list.iter() {
            let data = json!({
                "write": {
                    "command": "request",
                    "animName": effect_name,
                }
            });
            let Ok(res) = utils::request_put(
                &format!(
                    "http://{}:{}/api/v1/{}/effects/effectsList",
                    self.ip,
                    constants::NL_API_PORT,
                    self.token
                ),
                Some(&data),
            ) else {
                return Err(anyhow::Error::msg(format!(
                    "Couldn't reach the Nanoleaf device at {}.",
                    self.ip
                )));
            };
            let res_json: serde_json::Value = serde_json::from_str(&res)?;
            let palette_json = res_json["palette"].as_array().unwrap();
            let mut palette: Vec<Srgb<u8>> = Vec::new();
            for color_json in palette_json.iter() {
                let h = color_json["hue"].as_u64().unwrap() as f32;
                let s = (color_json["saturation"].as_u64().unwrap() as f32) / 100.0;
                let b = (color_json["brightness"].as_u64().unwrap() as f32) / 100.0;
                palette.push(Srgb::from_color(Hsv::new_srgb(h, s, b)).into_format::<u8>());
            }
            palettes.push(palette);
        }

        Ok(res_list
            .into_iter()
            .zip(palettes)
            .map(|x| Effect {
                name: x.0,
                colors: x.1,
            })
            .collect::<Vec<_>>())
    }

    pub fn play_effect(&self, effect_name: &str) -> Result<(), anyhow::Error> {
        let data = json!({
            "select": effect_name
        });
        let Ok(_) = utils::request_put(
            &format!(
                "http://{}:{}/api/v1/{}/effects",
                self.ip,
                constants::NL_API_PORT,
                self.token
            ),
            Some(&data),
        ) else {
            return Err(anyhow::Error::msg(format!(
                "Couldn't reach the Nanoleaf device at {}. Data: {}",
                self.ip, data
            )));
        };
        Ok(())
    }

    pub fn request_external_control(&self) -> Result<(), anyhow::Error> {
        let data = json!({
            "write": {
                "command": "display",
                "animType": "extControl",
                "extControlVersion": "v2"
            }
        });
        let Ok(_) = utils::request_put(
            &format!(
                "http://{}:{}/api/v1/{}/effects",
                self.ip,
                constants::NL_API_PORT,
                self.token
            ),
            Some(&data),
        ) else {
            return Err(anyhow::Error::msg(format!(
                "Couldn't reach the Nanoleaf device at {}. Data: {}",
                self.ip, data
            )));
        };
        Ok(())
    }

    /// Sort the primary axis according to the primary sorting order,
    /// and the secondary according to the secondary order
    pub fn sort_panels(&mut self, primary_axis: Axis, primary_sort: Sort, secondary_sort: Sort) {
        let sort_func = match primary_axis {
            Axis::X => match (primary_sort, secondary_sort) {
                (Sort::Asc, Sort::Asc) => {
                    |lhs: Panel, rhs: Panel| (lhs.x, lhs.y).cmp(&(rhs.x, rhs.y))
                }
                (Sort::Asc, Sort::Desc) => {
                    |lhs: Panel, rhs: Panel| (lhs.x, -lhs.y).cmp(&(rhs.x, -rhs.y))
                }
                (Sort::Desc, Sort::Asc) => {
                    |lhs: Panel, rhs: Panel| (-lhs.x, lhs.y).cmp(&(-rhs.x, rhs.y))
                }
                (Sort::Desc, Sort::Desc) => {
                    |lhs: Panel, rhs: Panel| (-lhs.x, -lhs.y).cmp(&(-rhs.x, -rhs.y))
                }
            },
            Axis::Y => match (primary_sort, secondary_sort) {
                (Sort::Asc, Sort::Asc) => {
                    |lhs: Panel, rhs: Panel| (lhs.y, lhs.x).cmp(&(rhs.y, rhs.x))
                }
                (Sort::Asc, Sort::Desc) => {
                    |lhs: Panel, rhs: Panel| (lhs.y, -lhs.x).cmp(&(rhs.y, -rhs.x))
                }
                (Sort::Desc, Sort::Asc) => {
                    |lhs: Panel, rhs: Panel| (-lhs.y, lhs.x).cmp(&(-rhs.y, rhs.x))
                }
                (Sort::Desc, Sort::Desc) => {
                    |lhs: Panel, rhs: Panel| (-lhs.y, -lhs.x).cmp(&(-rhs.y, -rhs.x))
                }
            },
        };
        self.panels
            .sort_by(|a: &Panel, b: &Panel| sort_func(*a, *b));
    }
}
