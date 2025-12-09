use anyhow::Result;
use serde_json::Value;

#[derive(Debug, Clone, Copy)]
pub struct ShapeType {
    pub id: u64,
    pub name: &'static str,
    pub side_length: f32,
}

impl ShapeType {
    /// Constructs a `ShapeType` from Nanoleaf's internal shape ID.
    ///
    /// Maps known IDs to panel types like triangles, squares, hexagons, controllers, and special shapes (e.g., Elements, Lines).
    /// Provides `side_length` approximation for rendering and `name` for display.
    /// Unknown IDs default to generic square with side 100.0.
    pub fn from_id(id: u64) -> Self {
        match id {
            0 => ShapeType {
                id,
                name: "Triangle",
                side_length: 150.0,
            },
            1 => ShapeType {
                id,
                name: "Rhythm",
                side_length: 0.0,
            },
            2 => ShapeType {
                id,
                name: "Square",
                side_length: 100.0,
            },
            3 => ShapeType {
                id,
                name: "Control Square Master",
                side_length: 100.0,
            },
            4 => ShapeType {
                id,
                name: "Control Square Passive",
                side_length: 100.0,
            },
            7 => ShapeType {
                id,
                name: "Hexagon (Shapes)",
                side_length: 67.0,
            },
            8 => ShapeType {
                id,
                name: "Triangle (Shapes)",
                side_length: 134.0,
            },
            9 => ShapeType {
                id,
                name: "Mini Triangle (Shapes)",
                side_length: 67.0,
            },
            12 => ShapeType {
                id,
                name: "Shapes Controller",
                side_length: 0.0,
            },
            14 => ShapeType {
                id,
                name: "Elements Hexagons",
                side_length: 134.0,
            },
            15 => ShapeType {
                id,
                name: "Elements Hexagons - Corner",
                side_length: 45.75,
            },
            16 => ShapeType {
                id,
                name: "Lines Connector",
                side_length: 11.0,
            },
            17 => ShapeType {
                id,
                name: "Light Lines",
                side_length: 154.0,
            },
            18 => ShapeType {
                id,
                name: "Light Lines - Single Zone",
                side_length: 77.0,
            },
            19 => ShapeType {
                id,
                name: "Controller Cap",
                side_length: 11.0,
            },
            20 => ShapeType {
                id,
                name: "Power Connector",
                side_length: 11.0,
            },
            29 => ShapeType {
                id,
                name: "Nanoleaf 4D Lightstrip",
                side_length: 50.0,
            },
            30 => ShapeType {
                id,
                name: "Skylight Panel",
                side_length: 180.0,
            },
            31 => ShapeType {
                id,
                name: "Skylight Controller Primary",
                side_length: 180.0,
            },
            32 => ShapeType {
                id,
                name: "Skylight Controller Passive Mode",
                side_length: 180.0,
            },
            _ => ShapeType {
                id,
                name: "Unknown",
                side_length: 100.0,
            },
        }
    }

    /// Returns the number of sides for this shape type, useful for polygon rendering.
    ///
    /// - 3 for triangles (IDs 0,8,9)
    /// - 4 for squares and most others (default)
    /// - 6 for hexagons (IDs 7,14,15)
    pub fn num_sides(&self) -> usize {
        match self.id {
            0 | 8 | 9 => 3,   // Triangles
            2..=4 => 4,       // Squares
            7 | 14 | 15 => 6, // Hexagons
            _ => 4,           // Default to square
        }
    }
}

#[derive(Debug)]
pub struct PanelInfo {
    pub panel_id: u16,
    pub x: i16,
    pub y: i16,
    pub orientation: u16,
    pub shape_type: ShapeType,
}

/// Parses the "positionData" array from Nanoleaf's panel layout JSON response.
///
/// Expects array of objects with "panelId" (u16), "x"/"y" (i16), "o" (orientation u16), "shapeType" (u64 ID).
/// Converts coordinates and creates `PanelInfo` for each, using `ShapeType::from_id`.
/// Defaults missing fields to 0.
///
/// # Arguments
///
/// * `layout_json` - serde_json::Value typically from /api/v1/panelLayout endpoint.
///
/// # Returns
///
/// `Result<Vec<PanelInfo>>` - List of parsed panel positions and types, or error if no positionData array.
pub fn parse_layout(layout_json: &Value) -> Result<Vec<PanelInfo>> {
    let position_data = layout_json["positionData"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("No positionData in layout"))?;

    let mut panels = Vec::new();

    for panel_data in position_data {
        let panel_id = panel_data["panelId"].as_u64().unwrap_or(0) as u16;
        let x = panel_data["x"].as_i64().unwrap_or(0) as i16;
        let y = panel_data["y"].as_i64().unwrap_or(0) as i16;
        let orientation = panel_data["o"].as_u64().unwrap_or(0) as u16;
        let shape_type_id = panel_data["shapeType"].as_u64().unwrap_or(0);
        let shape_type = ShapeType::from_id(shape_type_id);

        panels.push(PanelInfo {
            panel_id,
            x,
            y,
            orientation,
            shape_type,
        });
    }

    Ok(panels)
}

/// Prints a textual summary and table of the Nanoleaf panel layout to stdout.
///
/// Computes bounds (min/max x,y), displays global orientation, and tabulates:
/// Panel ID, Shape Type name, X/Y positions, Orientation (degrees), Side length.
///
/// Intended for CLI 'dump layout' command output. Suggests graphical alt for visual rep.
pub fn visualize_layout(panels: &[PanelInfo], global_orientation: u16) {
    if panels.is_empty() {
        println!("No panels to visualize");
        return;
    }

    // Find bounds
    let min_x = panels.iter().map(|p| p.x).min().unwrap();
    let max_x = panels.iter().map(|p| p.x).max().unwrap();
    let min_y = panels.iter().map(|p| p.y).min().unwrap();
    let max_y = panels.iter().map(|p| p.y).max().unwrap();

    println!("\n=== Panel Layout Visualization ===");
    println!("Global Orientation: {} degrees", global_orientation);
    println!("Bounds: X[{}, {}], Y[{}, {}]", min_x, max_x, min_y, max_y);
    println!("\nPanels:");
    println!(
        "{:<8} {:<30} {:<10} {:<10} {:<12} {:<12}",
        "Panel ID", "Shape Type", "X", "Y", "Orientation", "Side Length"
    );
    println!("{}", "-".repeat(100));

    for panel in panels {
        println!(
            "{:<8} {:<30} {:<10} {:<10} {:<12} {:<12.1}",
            panel.panel_id,
            panel.shape_type.name,
            panel.x,
            panel.y,
            format!("{}°", panel.orientation),
            panel.shape_type.side_length
        );
    }

    println!("\nNote: Use 'dump layout-graphical' for a visual representation");
}
