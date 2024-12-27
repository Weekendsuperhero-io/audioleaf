use crate::constants;
use crate::utils;
use palette::Hwb;
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
    id: u16,
    x: i16,
    y: i16,
}

#[derive(Debug)]
pub struct NanoleafDevice {
    pub ip: Ipv4Addr,
    pub name: String,
    pub panels: Vec<Panel>,
    state: bool,
    token: String,
}

#[derive(Debug, Default)]
struct Command {
    panel_no: usize,
    color: Hwb,
    transition_time: u16,
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
        let panels = Self::get_panels(&ip, &token)?;
        let state = Self::get_state(&ip, &token)?;

        Ok(NanoleafDevice {
            ip,
            name,
            panels,
            state,
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

    fn get_state(ip: &Ipv4Addr, token: &str) -> Result<bool, anyhow::Error> {
        let Ok(res) = utils::request_get(&format!(
            "http://{}:{}/api/v1/{}/state/on",
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
        Ok(res_json["value"].as_bool().unwrap())
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

    pub fn get_effect_list(&self) -> Result<Vec<String>, anyhow::Error> {
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
        Ok(res_list)
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

    // pub fn toggle_state(&self) -> Result<(), anyhow::Error> {
    //     if Self::get_state(&self.ip, &self.token)? {
    //         Ok(Self::turn_off())
    //     } else {
    //         Ok(Self::turn_on())
    //     }
    // }

    pub fn run_visualizer(&self, port: u16) -> Result<(), anyhow::Error> {
        Self::request_external_control(self)?;
        let socket = Self::enable_udp_socket(&self.ip, port);
        // spin up a thread for this
        Ok(())
    }

    fn request_external_control(&self) -> Result<(), anyhow::Error> {
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

    fn enable_udp_socket(ip: &Ipv4Addr, port: u16) -> Result<UdpSocket, anyhow::Error> {
        let socket_addr = SocketAddrV4::new(Ipv4Addr::new(0, 0, 0, 0), port);
        let socket = UdpSocket::bind(socket_addr)?;
        let nl_addr = SocketAddrV4::new(*ip, constants::NL_UDP_PORT);
        socket.connect(nl_addr)?;

        Ok(socket)
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

    // Run commands by sending bytes through UDP, see Nanoleaf API docs, section 3.2.6.2
    // pub fn run_commands(&self, commands: Vec<Command>) -> Result<(), anyhow::Error> {
    //     let split_into_bytes = |x: u16| -> (u8, u8) {
    //         // split a u16 into two bytes (in big endian), e.g. 651 -> (2, 139) because 651 = 2 * 256 + 139
    //         ((x / 256) as u8, (x % 256) as u8)
    //     };
    //
    //     let n_panels = commands.len();
    //     let mut buf = vec![0; 2];
    //     (buf[0], buf[1]) = split_into_bytes(n_panels as u16);
    //     for command in commands.iter() {
    //         let Command {
    //             panel_no,
    //             color: color_hwb,
    //             transition_time,
    //         } = command;
    //         let color_rgb = Srgb::from_color(*color_hwb).into_format::<u8>();
    //         let Srgb {
    //             red,
    //             green,
    //             blue,
    //             standard: _,
    //         } = color_rgb;
    //
    //         let mut sub_buf = [0u8; 8];
    //         (sub_buf[0], sub_buf[1]) = split_into_bytes(self.panels[*panel_no - 1].id);
    //         (sub_buf[2], sub_buf[3], sub_buf[4], sub_buf[5]) = (red, green, blue, 0);
    //         (sub_buf[6], sub_buf[7]) = split_into_bytes(*transition_time);
    //         buf.extend(sub_buf);
    //     }
    //     self.socket.send(&buf)?;
    //
    //     Ok(())
    // }
}
