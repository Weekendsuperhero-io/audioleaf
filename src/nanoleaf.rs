use crate::{
    config::{Axis, Sort},
    constants, utils,
};
use anyhow::{bail, Result};
use palette::{FromColor, Hsv, Srgb};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    fs::{File, OpenOptions},
    io::prelude::*,
    net::{Ipv4Addr, SocketAddrV4, UdpSocket},
    path::Path,
};

#[derive(Debug)]
pub struct NlEffect {
    pub name: String,
    pub palette: Vec<Srgb<u8>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NlDevice {
    pub name: String,
    pub ip: Ipv4Addr,
    pub token: String,
    #[serde(skip)]
    #[serde(default)]
    pub cur_effect_name: Option<String>,
}

/// wrapper struct for TOML serialization
#[derive(Debug, Serialize, Deserialize)]
struct NlDevices {
    nl_devices: Vec<NlDevice>,
}

impl From<Vec<NlDevice>> for NlDevices {
    /// Wraps a vector of devices into the TOML-serializable NlDevices struct.
    ///
    /// Used for saving/loading multiple known devices from nl_devices.toml file.
    fn from(nl_devices: Vec<NlDevice>) -> Self {
        NlDevices { nl_devices }
    }
}

impl NlDevice {
    /// Creates a new `NlDevice` instance for a given IP address.
    ///
    /// Performs API calls to:
    /// - Obtain auth token via POST /api/v1/new (requires device in pairing mode).
    /// - Fetch device name from GET /api/v1/{token}.
    /// - Get current effect name from GET /effects/select, mapping special names to None.
    ///
    /// # Arguments
    ///
    /// * `ip` - Local IPv4 address of the Nanoleaf device.
    ///
    /// # Returns
    ///
    /// `Result<NlDevice>` with name, ip, token, optional cur_effect_name.
    ///
    /// # Errors
    ///
    /// From HTTP requests or JSON parsing; bails on connection failure.
    pub fn new(ip: Ipv4Addr) -> Result<Self> {
        let token = Self::get_token(&ip)?;
        let name = Self::get_name(&ip, &token)?;
        let cur_effect_name = Self::get_cur_effect_name(&ip, &token)?;
        Ok(NlDevice {
            name,
            ip,
            token,
            cur_effect_name,
        })
    }

    fn get_token(ip: &Ipv4Addr) -> Result<String> {
        let Ok(res) = utils::request_post(
            &format!("http://{}:{}/api/v1/new", ip, constants::NL_API_PORT),
            None,
        ) else {
            bail!(utils::generate_connection_error_msg(ip));
        };
        let res_json: serde_json::Value = serde_json::from_str(&res)?;

        Ok(res_json["auth_token"].as_str().unwrap().trim().to_string())
    }

    fn get_name(ip: &Ipv4Addr, token: &str) -> Result<String> {
        let Ok(res) = utils::request_get(&format!(
            "http://{}:{}/api/v1/{}",
            ip,
            constants::NL_API_PORT,
            token
        )) else {
            bail!(utils::generate_connection_error_msg(ip));
        };
        let res_json: serde_json::Value = serde_json::from_str(&res)?;

        Ok(String::from(res_json["name"].as_str().unwrap()))
    }

    pub fn get_cur_effect_name(ip: &Ipv4Addr, token: &str) -> Result<Option<String>> {
        let Ok(res) = utils::request_get(&format!(
            "http://{}:{}/api/v1/{}/effects/select",
            ip,
            constants::NL_API_PORT,
            token
        )) else {
            bail!(utils::generate_connection_error_msg(ip));
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

    /// Retrieves the panel layout configuration from the device API.
    ///
    /// GET /api/v1/{token}/panelLayout/layout returns JSON with "positionData" array of panel positions/shapes.
    /// Used for layout visualization and panel sorting/indexing in UDP.
    ///
    /// # Returns
    ///
    /// `Result<serde_json::Value>` - Raw JSON response.
    ///
    /// # Errors
    ///
    /// HTTP or parsing errors, bails on connection fail.
    pub fn get_panel_layout(&self) -> Result<serde_json::Value> {
        let Ok(res) = utils::request_get(&format!(
            "http://{}:{}/api/v1/{}/panelLayout/layout",
            self.ip,
            constants::NL_API_PORT,
            self.token
        )) else {
            bail!(utils::generate_connection_error_msg(&self.ip));
        };
        let res_json: serde_json::Value = serde_json::from_str(&res)?;
        Ok(res_json)
    }

    pub fn get_global_orientation(&self) -> Result<serde_json::Value> {
        let Ok(res) = utils::request_get(&format!(
            "http://{}:{}/api/v1/{}/panelLayout/globalOrientation",
            self.ip,
            constants::NL_API_PORT,
            self.token
        )) else {
            bail!(utils::generate_connection_error_msg(&self.ip));
        };
        let res_json: serde_json::Value = serde_json::from_str(&res)?;
        Ok(res_json)
    }

    pub fn get_device_info(&self) -> Result<serde_json::Value> {
        let Ok(res) = utils::request_get(&format!(
            "http://{}:{}/api/v1/{}/",
            self.ip,
            constants::NL_API_PORT,
            self.token
        )) else {
            bail!(utils::generate_connection_error_msg(&self.ip));
        };
        let res_json: serde_json::Value = serde_json::from_str(&res)?;
        Ok(res_json)
    }

    pub fn set_state(&self, power_on: Option<bool>, brightness: Option<u8>) -> Result<()> {
        let mut state = serde_json::Map::new();

        if let Some(on) = power_on {
            let mut on_obj = serde_json::Map::new();
            on_obj.insert("value".to_string(), serde_json::Value::Bool(on));
            state.insert("on".to_string(), serde_json::Value::Object(on_obj));
        }

        if let Some(brightness_val) = brightness {
            let mut brightness_obj = serde_json::Map::new();
            brightness_obj.insert(
                "value".to_string(),
                serde_json::Value::Number(brightness_val.into()),
            );
            state.insert(
                "brightness".to_string(),
                serde_json::Value::Object(brightness_obj),
            );
        }

        if state.is_empty() {
            return Ok(());
        }

        let data = serde_json::Value::Object(state);
        let Ok(_) = utils::request_put(
            &format!(
                "http://{}:{}/api/v1/{}/state",
                self.ip,
                constants::NL_API_PORT,
                self.token
            ),
            Some(&data),
        ) else {
            bail!(utils::generate_connection_error_msg(&self.ip));
        };
        Ok(())
    }

    pub fn ensure_device_ready(&self) -> Result<()> {
        let info = self.get_device_info()?;

        let is_on = info["state"]["on"]["value"].as_bool().unwrap_or(true);
        let brightness = info["state"]["brightness"]["value"].as_u64().unwrap_or(100) as u8;

        let mut needs_power = false;
        let mut needs_brightness = false;

        if !is_on {
            eprintln!("Device is off. Turning on...");
            needs_power = true;
        }

        if brightness == 0 {
            eprintln!("Device brightness is 0. Setting to 100...");
            needs_brightness = true;
        }

        if needs_power || needs_brightness {
            self.set_state(
                if needs_power { Some(true) } else { None },
                if needs_brightness { Some(100) } else { None },
            )?;
            // Give the device a moment to respond to the state change
            std::thread::sleep(std::time::Duration::from_millis(500));
        }

        Ok(())
    }

    pub fn get_panels(&self) -> Result<Vec<Panel>> {
        let res_json = self.get_panel_layout()?;
        let res_panels = res_json["positionData"].as_array().unwrap();
        let mut panels = Vec::new();
        for panel in res_panels.iter() {
            let id = panel["panelId"].as_u64().unwrap() as u16;
            let shape_type = panel["shapeType"].as_u64().unwrap_or(0);

            // Filter out controller units (shapeType 12) and other non-light panels
            // shapeType 0-11 are actual light panels (Canvas squares, Shapes triangles, etc.)
            // shapeType 12+ are controllers and other components
            if shape_type >= 12 {
                continue;
            }

            let (x, y) = (
                panel["x"].as_i64().unwrap() as i16,
                panel["y"].as_i64().unwrap() as i16,
            );
            panels.push(Panel { id, x, y });
        }

        Ok(panels)
    }

    pub fn get_effect_list(&self) -> Result<Vec<NlEffect>> {
        let Ok(res) = utils::request_get(&format!(
            "http://{}:{}/api/v1/{}/effects/effectsList",
            self.ip,
            constants::NL_API_PORT,
            self.token
        )) else {
            bail!(utils::generate_connection_error_msg(&self.ip));
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
                bail!(utils::generate_connection_error_msg(&self.ip));
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
            .map(|x| NlEffect {
                name: x.0,
                palette: x.1,
            })
            .collect::<Vec<_>>())
    }

    pub fn play_effect(&self, effect_name: &str) -> Result<()> {
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
            bail!(utils::generate_connection_error_msg(&self.ip));
        };
        Ok(())
    }

    pub fn get_udp_socket(&self) -> Result<UdpSocket> {
        let socket = UdpSocket::bind("0.0.0.0:0")?;
        socket.connect(SocketAddrV4::new(self.ip, constants::NL_UDP_PORT))?;

        Ok(socket)
    }

    pub fn request_udp_control(&self) -> Result<()> {
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
            bail!(utils::generate_connection_error_msg(&self.ip));
        };
        Ok(())
    }

    pub fn find_in_file(path: &Path, device_name: Option<&str>) -> Result<Self> {
        let mut devices_file = File::open(path)?;
        let mut contents = String::new();
        devices_file.read_to_string(&mut contents)?;
        let devices: NlDevices = toml::from_str(&contents)?;
        let devices = devices.nl_devices;

        if devices.is_empty() {
            bail!(format!("devices file {} is empty", path.to_string_lossy()));
        }
        let Some(device_name) = device_name else {
            return Ok(devices.into_iter().next().unwrap());
        };
        match devices
            .into_iter()
            .find(|device| device.name.as_str() == device_name)
        {
            Some(device) => Ok(device),
            None => bail!(format!("Nanoleaf device `{}` not found", device_name)),
        }
    }

    pub fn append_to_file(&self, path: &Path) -> Result<()> {
        // Create parent directory if it doesn't exist
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut devices_file = OpenOptions::new().append(true).create(true).open(path)?;
        let data: String = toml::to_string_pretty(self)?;
        writeln!(devices_file, "[[nl_devices]]")?;
        writeln!(devices_file, "{}", data)?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Panel {
    pub id: u16,
    pub x: i16,
    pub y: i16,
}

#[derive(Debug)]
pub struct NlUdp {
    pub socket: UdpSocket,
    pub panels: Vec<Panel>,
}

impl NlUdp {
    pub fn new(nl_device: &NlDevice) -> Result<Self> {
        Ok(NlUdp {
            socket: nl_device.get_udp_socket()?,
            panels: nl_device.get_panels()?,
        })
    }

    pub fn sort_panels_with_orientation(
        &mut self,
        primary_axis: Option<Axis>,
        primary_sort: Option<Sort>,
        secondary_sort: Option<Sort>,
        global_orientation: u16,
    ) {
        let primary_axis = primary_axis.unwrap_or_default();
        let primary_sort = primary_sort.unwrap_or_default();
        let secondary_sort = secondary_sort.unwrap_or_default();

        // Apply global orientation rotation to coordinates if needed
        let angle = -(global_orientation as f32).to_radians();
        let needs_rotation = global_orientation != 0;

        let sort_func = match primary_axis {
            Axis::X => match (primary_sort, secondary_sort) {
                (Sort::Asc, Sort::Asc) => |lhs: &Panel, rhs: &Panel, angle: f32, rotate: bool| {
                    if rotate {
                        let (lx, ly) = Self::rotate_coords(lhs.x, lhs.y, angle);
                        let (rx, ry) = Self::rotate_coords(rhs.x, rhs.y, angle);
                        (lx, ly).partial_cmp(&(rx, ry)).unwrap()
                    } else {
                        (lhs.x, lhs.y).cmp(&(rhs.x, rhs.y))
                    }
                },
                (Sort::Asc, Sort::Desc) => |lhs: &Panel, rhs: &Panel, angle: f32, rotate: bool| {
                    if rotate {
                        let (lx, ly) = Self::rotate_coords(lhs.x, lhs.y, angle);
                        let (rx, ry) = Self::rotate_coords(rhs.x, rhs.y, angle);
                        (lx, -ly).partial_cmp(&(rx, -ry)).unwrap()
                    } else {
                        (lhs.x, -lhs.y).cmp(&(rhs.x, -rhs.y))
                    }
                },
                (Sort::Desc, Sort::Asc) => |lhs: &Panel, rhs: &Panel, angle: f32, rotate: bool| {
                    if rotate {
                        let (lx, ly) = Self::rotate_coords(lhs.x, lhs.y, angle);
                        let (rx, ry) = Self::rotate_coords(rhs.x, rhs.y, angle);
                        (-lx, ly).partial_cmp(&(-rx, ry)).unwrap()
                    } else {
                        (-lhs.x, lhs.y).cmp(&(-rhs.x, rhs.y))
                    }
                },
                (Sort::Desc, Sort::Desc) => |lhs: &Panel, rhs: &Panel, angle: f32, rotate: bool| {
                    if rotate {
                        let (lx, ly) = Self::rotate_coords(lhs.x, lhs.y, angle);
                        let (rx, ry) = Self::rotate_coords(rhs.x, rhs.y, angle);
                        (-lx, -ly).partial_cmp(&(-rx, -ry)).unwrap()
                    } else {
                        (-lhs.x, -lhs.y).cmp(&(-rhs.x, -rhs.y))
                    }
                },
            },
            Axis::Y => match (primary_sort, secondary_sort) {
                (Sort::Asc, Sort::Asc) => |lhs: &Panel, rhs: &Panel, angle: f32, rotate: bool| {
                    if rotate {
                        let (lx, ly) = Self::rotate_coords(lhs.x, lhs.y, angle);
                        let (rx, ry) = Self::rotate_coords(rhs.x, rhs.y, angle);
                        (ly, lx).partial_cmp(&(ry, rx)).unwrap()
                    } else {
                        (lhs.y, lhs.x).cmp(&(rhs.y, rhs.x))
                    }
                },
                (Sort::Asc, Sort::Desc) => |lhs: &Panel, rhs: &Panel, angle: f32, rotate: bool| {
                    if rotate {
                        let (lx, ly) = Self::rotate_coords(lhs.x, lhs.y, angle);
                        let (rx, ry) = Self::rotate_coords(rhs.x, rhs.y, angle);
                        (ly, -lx).partial_cmp(&(ry, -rx)).unwrap()
                    } else {
                        (lhs.y, -lhs.x).cmp(&(rhs.y, -rhs.x))
                    }
                },
                (Sort::Desc, Sort::Asc) => |lhs: &Panel, rhs: &Panel, angle: f32, rotate: bool| {
                    if rotate {
                        let (lx, ly) = Self::rotate_coords(lhs.x, lhs.y, angle);
                        let (rx, ry) = Self::rotate_coords(rhs.x, rhs.y, angle);
                        (-ly, lx).partial_cmp(&(-ry, rx)).unwrap()
                    } else {
                        (-lhs.y, lhs.x).cmp(&(-rhs.y, rhs.x))
                    }
                },
                (Sort::Desc, Sort::Desc) => |lhs: &Panel, rhs: &Panel, angle: f32, rotate: bool| {
                    if rotate {
                        let (lx, ly) = Self::rotate_coords(lhs.x, lhs.y, angle);
                        let (rx, ry) = Self::rotate_coords(rhs.x, rhs.y, angle);
                        (-ly, -lx).partial_cmp(&(-ry, -rx)).unwrap()
                    } else {
                        (-lhs.y, -lhs.x).cmp(&(-rhs.y, -rhs.x))
                    }
                },
            },
        };
        self.panels
            .sort_by(|a: &Panel, b: &Panel| sort_func(a, b, angle, needs_rotation));
    }

    fn rotate_coords(x: i16, y: i16, angle: f32) -> (i32, i32) {
        let x_f = x as f32;
        let y_f = y as f32;
        let rotated_x = (x_f * angle.cos() - y_f * angle.sin()).round() as i32;
        let rotated_y = (x_f * angle.sin() + y_f * angle.cos()).round() as i32;
        (rotated_x, rotated_y)
    }

    pub fn update_panels(&self, colors: &[palette::Hwb], trans_time: i16) -> Result<()> {
        let mut buf = vec![0; 8 * self.panels.len() + 2];
        (buf[0], buf[1]) = utils::split_into_bytes(self.panels.len() as u16);
        for (i, color) in colors.iter().enumerate() {
            let Srgb {
                red: r,
                green: g,
                blue: b,
                ..
            } = palette::Srgb::from_color(*color).into_format::<u8>();
            let offset = 8 * i + 2;
            (buf[offset], buf[offset + 1]) = utils::split_into_bytes(self.panels[i].id);
            (
                buf[offset + 2],
                buf[offset + 3],
                buf[offset + 4],
                buf[offset + 5],
            ) = (r, g, b, 0);
            // Convert i16 to bytes (supporting -1 for instant transition)
            // trans_time is in units of 100ms: 1 = 100ms, 2 = 200ms, -1 = instant
            let trans_time_u16 = if trans_time == -1 {
                0xFFFF // -1 as u16 (two's complement)
            } else {
                trans_time as u16
            };
            (buf[offset + 6], buf[offset + 7]) = utils::split_into_bytes(trans_time_u16);
        }
        self.socket.send(&buf)?;

        Ok(())
    }
}
