use crate::layout_visualizer::PanelInfo;
use crate::nanoleaf::NlDevice;
use macroquad::prelude::*;
use palette::Hwb;
use std::f32::consts::PI;
use std::thread;
use std::time::Duration;

pub fn visualize_graphical(panels: Vec<PanelInfo>, global_orientation: u16, device: NlDevice) {
    // Run the visualization synchronously
    pollster::block_on(visualize_async(panels, global_orientation, device));
}

async fn visualize_async(panels: Vec<PanelInfo>, global_orientation: u16, device: NlDevice) {
    // Initialize macroquad window
    macroquad::Window::new("Nanoleaf Panel Layout", async move {
        visualize_loop(panels, global_orientation, device).await;
    });
}

async fn visualize_loop(panels: Vec<PanelInfo>, global_orientation: u16, device: NlDevice) {
    // Find bounds
    let min_x = panels.iter().map(|p| p.x).min().unwrap_or(0) as f32;
    let max_x = panels.iter().map(|p| p.x).max().unwrap_or(0) as f32;
    let min_y = panels.iter().map(|p| p.y).min().unwrap_or(0) as f32;
    let max_y = panels.iter().map(|p| p.y).max().unwrap_or(0) as f32;

    let layout_width = max_x - min_x;
    let layout_height = max_y - min_y;

    // Window configuration
    let window_width = 1200.0;
    let window_height = 800.0;

    // Calculate scale to fit layout in window with padding
    let padding_top = 100.0; // Extra space at top for title
    let padding_bottom = 50.0;
    let padding_sides = 50.0;
    let available_width = window_width - 2.0 * padding_sides;
    let available_height = window_height - padding_top - padding_bottom;

    let scale_x = available_width / layout_width;
    let scale_y = available_height / layout_height;
    let scale = scale_x.min(scale_y);

    // Setup Nanoleaf controller for sending commands
    let nl_controller = match crate::nanoleaf::NlUdp::new(&device) {
        Ok(controller) => Some(controller),
        Err(e) => {
            eprintln!("Warning: Could not initialize Nanoleaf controller: {}", e);
            None
        }
    };

    loop {
        clear_background(Color::from_rgba(20, 20, 30, 255));

        // Draw title
        draw_text(
            &format!(
                "Nanoleaf Panel Layout - Global Orientation: {}°",
                global_orientation
            ),
            10.0,
            30.0,
            30.0,
            WHITE,
        );

        // Center the layout in the window horizontally, offset from top
        let offset_x = (window_width - layout_width * scale) / 2.0;
        let offset_y = padding_top + (available_height - layout_height * scale) / 2.0;

        // First pass: calculate all transformed positions
        let mut transformed_positions = Vec::new();
        for panel in &panels {
            // Apply global orientation rotation to coordinates
            let rel_x = (panel.x as f32 - min_x) - layout_width / 2.0;
            let rel_y = (panel.y as f32 - min_y) - layout_height / 2.0;

            let angle = -(global_orientation as f32).to_radians(); // Negative for clockwise
            let rotated_x = rel_x * angle.cos() - rel_y * angle.sin();
            let rotated_y = rel_x * angle.sin() + rel_y * angle.cos();

            // Convert to screen coordinates
            let screen_x = offset_x + (rotated_x + layout_width / 2.0) * scale;
            let screen_y = offset_y + (layout_height / 2.0 - rotated_y) * scale; // Flip Y

            transformed_positions.push((screen_x, screen_y));
        }

        // Second pass: draw all panels with access to all positions
        for (i, panel) in panels.iter().enumerate() {
            let (screen_x, screen_y) = transformed_positions[i];
            draw_panel(
                screen_x,
                screen_y,
                panel,
                scale,
                &panels,
                &transformed_positions,
            );
        }

        // Handle mouse clicks
        if is_mouse_button_pressed(MouseButton::Left) && nl_controller.is_some() {
            let (mouse_x, mouse_y) = mouse_position();

            // Check which panel was clicked
            for (i, panel) in panels.iter().enumerate() {
                if panel.shape_type.side_length < 1.0 {
                    continue; // Skip controllers
                }

                let (screen_x, screen_y) = transformed_positions[i];
                let num_sides = panel.shape_type.num_sides();
                let side_length = panel.shape_type.side_length * scale;

                let radius = if num_sides == 3 {
                    side_length / f32::sqrt(3.0)
                } else if num_sides == 4 {
                    side_length / f32::sqrt(2.0)
                } else {
                    side_length
                };

                // Simple distance check for click detection
                let dist = ((mouse_x - screen_x).powi(2) + (mouse_y - screen_y).powi(2)).sqrt();
                if dist < radius * 1.2 {
                    // Found the clicked panel - flash it
                    if let Some(ref controller) = nl_controller {
                        flash_panel(controller, &panels, panel.panel_id);
                    }
                    break;
                }
            }
        }

        // Instructions
        draw_text(
            "Press ESC to close | Click panels to flash them",
            10.0,
            window_height - 20.0,
            20.0,
            GRAY,
        );

        if is_key_pressed(KeyCode::Escape) {
            break;
        }

        next_frame().await
    }
}

fn draw_panel(
    x: f32,
    y: f32,
    panel: &PanelInfo,
    scale: f32,
    all_panels: &[PanelInfo],
    transformed_positions: &[(f32, f32)],
) {
    // Handle controllers specially (they have side_length 0)
    if panel.shape_type.side_length < 1.0 {
        // Find the nearest panel to attach to
        let mut min_dist = f32::MAX;
        let mut nearest_idx = 0;

        for (i, other_panel) in all_panels.iter().enumerate() {
            if other_panel.shape_type.side_length >= 1.0 {
                let (other_x, other_y) = transformed_positions[i];
                let dist = ((x - other_x).powi(2) + (y - other_y).powi(2)).sqrt();
                if dist < min_dist {
                    min_dist = dist;
                    nearest_idx = i;
                }
            }
        }

        let (parent_x, parent_y) = transformed_positions[nearest_idx];
        let parent_panel = &all_panels[nearest_idx];

        // Calculate angle from parent to controller
        let dx = x - parent_x;
        let dy = y - parent_y;
        let angle_to_controller = dy.atan2(dx);

        // Get parent shape info
        let num_sides = parent_panel.shape_type.num_sides();
        let parent_side_length = parent_panel.shape_type.side_length * scale;

        // Calculate parent radius
        let parent_radius = if num_sides == 3 {
            parent_side_length / f32::sqrt(3.0)
        } else if num_sides == 4 {
            parent_side_length / f32::sqrt(2.0)
        } else {
            parent_side_length
        };

        // Find which edge of the parent the controller is closest to
        let parent_orientation = (parent_panel.orientation as f32).to_radians();
        let angle_per_side = 2.0 * PI / num_sides as f32;

        let mut closest_edge = 0;
        let mut min_angle_diff = f32::MAX;

        for i in 0..num_sides {
            let vertex_angle = parent_orientation + (i as f32 * angle_per_side);
            let angle_diff = ((angle_to_controller - vertex_angle).abs() % (2.0 * PI))
                .min((2.0 * PI) - ((angle_to_controller - vertex_angle).abs() % (2.0 * PI)));
            if angle_diff < min_angle_diff {
                min_angle_diff = angle_diff;
                closest_edge = i;
            }
        }

        // Calculate the two vertices of the edge
        let v1_angle = parent_orientation + (closest_edge as f32 * angle_per_side);
        let v2_angle = parent_orientation + ((closest_edge + 1) as f32 * angle_per_side);

        let v1_x = parent_x + parent_radius * v1_angle.cos();
        let v1_y = parent_y + parent_radius * v1_angle.sin();
        let v2_x = parent_x + parent_radius * v2_angle.cos();
        let v2_y = parent_y + parent_radius * v2_angle.sin();

        // Draw trapezoid attached to this edge
        let trapezoid_height = 20.0;

        // Calculate perpendicular direction (outward from parent)
        let edge_mid_x = (v1_x + v2_x) / 2.0;
        let edge_mid_y = (v1_y + v2_y) / 2.0;
        let perp_dx = edge_mid_x - parent_x;
        let perp_dy = edge_mid_y - parent_y;
        let perp_len = (perp_dx * perp_dx + perp_dy * perp_dy).sqrt();
        let perp_norm_x = perp_dx / perp_len;
        let perp_norm_y = perp_dy / perp_len;

        // Trapezoid vertices: top edge matches parent edge, bottom edge is narrower
        let narrow_ratio = 0.6; // Bottom edge is 60% of top edge

        let vertices = [
            Vec2::new(v1_x, v1_y), // Top left (on parent edge)
            Vec2::new(v2_x, v2_y), // Top right (on parent edge)
            // Bottom right (narrower, extended outward)
            Vec2::new(
                v2_x + perp_norm_x * trapezoid_height - (v2_x - edge_mid_x) * (1.0 - narrow_ratio),
                v2_y + perp_norm_y * trapezoid_height - (v2_y - edge_mid_y) * (1.0 - narrow_ratio),
            ),
            // Bottom left (narrower, extended outward)
            Vec2::new(
                v1_x + perp_norm_x * trapezoid_height - (v1_x - edge_mid_x) * (1.0 - narrow_ratio),
                v1_y + perp_norm_y * trapezoid_height - (v1_y - edge_mid_y) * (1.0 - narrow_ratio),
            ),
        ];

        // Draw filled trapezoid
        draw_triangle(
            vertices[0],
            vertices[1],
            vertices[2],
            Color::from_rgba(255, 200, 0, 255),
        );
        draw_triangle(
            vertices[0],
            vertices[2],
            vertices[3],
            Color::from_rgba(255, 200, 0, 255),
        );

        // Draw outline
        for i in 0..vertices.len() {
            let next = (i + 1) % vertices.len();
            draw_line(
                vertices[i].x,
                vertices[i].y,
                vertices[next].x,
                vertices[next].y,
                2.0,
                Color::from_rgba(200, 150, 0, 255),
            );
        }

        // Draw "C" label in center of trapezoid
        let text_size = 10.0;
        let text_dims = measure_text("C", None, text_size as u16, 1.0);
        let label_x = (vertices[0].x + vertices[1].x + vertices[2].x + vertices[3].x) / 4.0;
        let label_y = (vertices[0].y + vertices[1].y + vertices[2].y + vertices[3].y) / 4.0;
        draw_text(
            "C",
            label_x - text_dims.width / 2.0,
            label_y + text_size / 3.0,
            text_size,
            BLACK,
        );
        return;
    }

    let num_sides = panel.shape_type.num_sides();
    let side_length = panel.shape_type.side_length * scale;

    // Calculate radius from center to vertex
    let radius = if num_sides == 3 {
        side_length / f32::sqrt(3.0)
    } else if num_sides == 4 {
        side_length / f32::sqrt(2.0)
    } else {
        side_length
    };

    // Calculate vertices
    let start_angle = (panel.orientation as f32).to_radians();
    let mut vertices = Vec::new();

    for i in 0..num_sides {
        let angle = start_angle + (i as f32 * 2.0 * PI / num_sides as f32);
        let vx = x + radius * angle.cos();
        let vy = y + radius * angle.sin();
        vertices.push(Vec2::new(vx, vy));
    }

    // Choose color based on shape type
    let color = match panel.shape_type.id {
        0 | 8 | 9 => Color::from_rgba(255, 100, 100, 200),
        2..=4 => Color::from_rgba(100, 255, 100, 200),
        7 | 14 | 15 => Color::from_rgba(100, 150, 255, 200),
        30..=32 => Color::from_rgba(255, 255, 100, 200),
        _ => Color::from_rgba(150, 150, 150, 200),
    };

    // Draw filled polygon
    for i in 1..(num_sides - 1) {
        draw_triangle(vertices[0], vertices[i], vertices[i + 1], color);
    }

    // Draw outline
    for i in 0..num_sides {
        let next = (i + 1) % num_sides;
        draw_line(
            vertices[i].x,
            vertices[i].y,
            vertices[next].x,
            vertices[next].y,
            2.0,
            WHITE,
        );
    }

    // Draw panel ID in center
    let id_text = format!("{}", panel.panel_id);
    let text_size = 16.0;
    let text_dims = measure_text(&id_text, None, text_size as u16, 1.0);
    draw_text(
        &id_text,
        x - text_dims.width / 2.0,
        y + text_size / 3.0,
        text_size,
        BLACK,
    );
}

fn flash_panel(
    controller: &crate::nanoleaf::NlUdp,
    all_panels: &[PanelInfo],
    clicked_panel_id: u16,
) {
    // Create color array - white for clicked panel, black for all others
    // Only include actual light panels (skip controllers with side_length < 1.0)
    let colors: Vec<Hwb> = all_panels
        .iter()
        .filter(|panel| panel.shape_type.side_length >= 1.0)
        .map(|panel| {
            if panel.panel_id == clicked_panel_id {
                Hwb::new(0.0, 1.0, 0.0) // White
            } else {
                Hwb::new(0.0, 0.0, 1.0) // Black
            }
        })
        .collect();

    // Flash on
    let _ = controller.update_panels(&colors, 1);

    // Brief delay
    thread::sleep(Duration::from_millis(300));

    // Flash off - set all panels to black to return to normal
    let black_colors: Vec<Hwb> = all_panels
        .iter()
        .filter(|panel| panel.shape_type.side_length >= 1.0)
        .map(|_| Hwb::new(0.0, 0.0, 1.0))
        .collect();
    let _ = controller.update_panels(&black_colors, 1);
}
