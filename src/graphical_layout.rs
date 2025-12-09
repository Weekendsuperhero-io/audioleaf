use crate::layout_visualizer::PanelInfo;
use crate::nanoleaf::NlDevice;
use macroquad::prelude::*;
use palette::Hwb;
use std::f32::consts::PI;
use std::thread;
use std::time::Duration;

/// graphical_layout - Graphical Visualization of Nanoleaf Panel Layouts
///
/// This module provides an interactive graphical interface to visualize and interact with
/// Nanoleaf panel layouts using the macroquad rendering engine. It renders panels as
/// polygons scaled to fit the window, applies global orientation rotations, and supports
/// mouse interaction for flashing individual panels via UDP commands.
///
/// ## Main Components
///
/// - `visualize_graphical`: Entry point function to launch the visualization window.
/// - `visualize_loop`: Core async loop handling rendering and input.
/// - `draw_panel`: Renders individual panels or controller trapezoids.
/// - `flash_panel`: Sends UDP color updates to flash a panel white briefly.
///
/// ## Color Mapping for Panels
///
/// Panels are colored by shape family:
/// - Triangles (IDs 0,8,9): Red-ish
/// - Squares (2-4): Green-ish
/// - Hexagons (7,14,15): Blue-ish
/// - Skylight panels (30-32): Yellow-ish
/// - Others: Gray
///
/// Controllers are always yellow trapezoids labeled "C".
///
/// ## Interaction
///
/// - Left-click a panel to flash it.
/// - ESC to exit.
/// - Window auto-scales layout with padding.
///
/// ## Error Handling
///
/// Warns if UDP controller fails to initialize but continues without interaction.
/// Relies on `pollster::block_on` for async compatibility.
/// Visualizes the Nanoleaf panel layout in a graphical window using the macroquad game engine.
///
/// This function creates an interactive window displaying the physical arrangement of Nanoleaf panels.
/// Panels are rendered as colored polygons based on their shape type (triangles, squares, hexagons).
/// Controller panels are depicted as yellow trapezoids attached to nearby light panels.
/// The layout can be rotated according to the global orientation.
/// Users can click on panels to briefly flash them white using UDP commands to the device.
///
/// The window includes:
/// - Title showing global orientation
/// - Scaled and centered layout
/// - Panel IDs labeled in centers
/// - Instructions for interaction
///
/// Press ESC to close the window.
///
/// # Arguments
///
/// * `panels` - Vector of `PanelInfo` structs describing each panel's position, orientation, and shape.
/// * `global_orientation` - The global rotation of the layout in degrees (u16).
/// * `device` - `NlDevice` containing IP and auth token for UDP communication.
///
/// # Panics
///
/// Panics if macroquad window creation or async runtime fails.
///
/// # Examples
///
/// ```
/// // Assuming panels and device are obtained from layout parsing
/// visualize_graphical(panels, global_orientation, device);
/// ```
///
/// # Dependencies
///
/// Requires `macroquad` and `palette` crates for rendering and color handling.
pub fn visualize_graphical(panels: Vec<PanelInfo>, global_orientation: u16, device: NlDevice) {
    // Synchronously block on the async visualization routine using pollster::block_on,
    // bridging the synchronous entry point to macroquad's async window and event loop.
    pollster::block_on(visualize_async(panels, global_orientation, device));
}

/// Asynchronous function that sets up the macroquad window and runs the visualization loop.
///
/// This private helper function is called by `visualize_graphical` to handle the async nature
/// of macroquad's event loop. It creates a window titled "Nanoleaf Panel Layout" and awaits
/// the main loop execution.
///
/// # Arguments
///
/// * `panels` - The panel layout data.
/// * `global_orientation` - Device global orientation.
/// * `device` - Nanoleaf device for interaction.
async fn visualize_async(panels: Vec<PanelInfo>, global_orientation: u16, device: NlDevice) {
    // Create and configure the macroquad window with fixed size and title,
    // then spawn the async block to run the main visualization loop.
    macroquad::Window::new("Nanoleaf Panel Layout", async move {
        visualize_loop(panels, global_orientation, device).await;
    });
}

/// The core asynchronous loop that runs the interactive visualization.
///
/// This function implements the main game loop:
/// - Clears background and draws title.
/// - Computes transformed positions applying global rotation.
/// - Draws all panels and controllers.
/// - Handles left mouse clicks to flash panels via UDP if controller available.
/// - Draws instructions and checks for ESC key to exit.
///
/// # Arguments
///
/// * `panels` - List of all panels including controllers.
/// * `global_orientation` - Applied as clockwise rotation to layout.
/// * `device` - Used to create UDP controller for flashing.
async fn visualize_loop(panels: Vec<PanelInfo>, global_orientation: u16, device: NlDevice) {
    // Calculate the layout bounds by finding min/max coordinates of all panels,
    // used for scaling and centering the visualization.
    let min_x = panels.iter().map(|p| p.x).min().unwrap_or(0) as f32;
    let max_x = panels.iter().map(|p| p.x).max().unwrap_or(0) as f32;
    let min_y = panels.iter().map(|p| p.y).min().unwrap_or(0) as f32;
    let max_y = panels.iter().map(|p| p.y).max().unwrap_or(0) as f32;

    let layout_width = max_x - min_x;
    let layout_height = max_y - min_y;

    // Set fixed window size: 1200x800 pixels for optimal layout display.
    let window_width = 1200.0;
    let window_height = 800.0;

    // Calculate uniform scaling factor to fit the layout inside the window with padding,
    // using the minimum of horizontal and vertical scales to avoid distortion.
    let padding_top = 100.0; // Extra space at top for title
    let padding_bottom = 50.0;
    let padding_sides = 50.0;
    let available_width = window_width - 2.0 * padding_sides;
    let available_height = window_height - padding_top - padding_bottom;

    let scale_x = available_width / layout_width;
    let scale_y = available_height / layout_height;
    let scale = scale_x.min(scale_y);

    // Initialize optional UDP controller using device IP and token for flashing panels.
    // Gracefully handles initialization failure by disabling clicks but continuing render.
    let nl_controller = match crate::nanoleaf::NlUdp::new(&device) {
        Ok(controller) => Some(controller),
        Err(e) => {
            eprintln!("Warning: Could not initialize Nanoleaf controller: {}", e);
            None
        }
    };

    loop {
        clear_background(Color::from_rgba(20, 20, 30, 255));

        // Draw dynamic title displaying global orientation at top-left with white text.
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

        // Calculate screen offsets to center the scaled layout horizontally,
        // vertically centered within available height below title padding.
        let offset_x = (window_width - layout_width * scale) / 2.0;
        let offset_y = padding_top + (available_height - layout_height * scale) / 2.0;

        // First pass: Precompute screen positions for all panels.
        // - Translate to layout-relative coords centered at (0,0)
        // - Rotate clockwise by -global_orientation radians around origin
        // - Translate back and scale to screen coordinates with offsets
        // This enables two-pass rendering: positions first, then drawing with full context.
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

        // Second pass: Draw each panel using transformed positions, providing full layout
        // context needed for controller attachment calculations.
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

        // Handle user interaction: On left mouse press with valid controller,
        // check distance from mouse to each panel center; if within ~1.2x radius, flash it.
        if is_mouse_button_pressed(MouseButton::Left) && nl_controller.is_some() {
            let (mouse_x, mouse_y) = mouse_position();

            // Scan panels for mouse hit detection, skipping controllers (no side_length).
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

                // Perform radial distance check: mouse within 120% of estimated panel radius
                // (approximated from side_length and shape: tri/sqrt(3), sq/sqrt(2), else side).
                let dist = ((mouse_x - screen_x).powi(2) + (mouse_y - screen_y).powi(2)).sqrt();
                if dist < radius * 1.2 {
                    // Panel hit confirmed: send flash command via UDP controller and exit loop.
                    if let Some(ref controller) = nl_controller {
                        flash_panel(controller, &panels, panel.panel_id);
                    }
                    break;
                }
            }
        }

        // Render interaction instructions at bottom-left in smaller gray text.
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

/// Draws a single panel or controller at the specified screen position.
///
/// Supports different shape types:
/// - Light panels (side_length >=1): Polygons (triangles=3 sides, squares=4, hex=6) with colors based on shape ID.
/// - Controllers (side_length <1): Yellow trapezoids attached to the nearest light panel's edge.
///
/// For light panels:
/// - Vertices calculated from radius, orientation.
/// - Filled with semi-transparent color matching shape family.
/// - White outline.
/// - Panel ID text in center.
///
/// For controllers:
/// - Finds nearest parent panel.
/// - Determines closest edge.
/// - Draws trapezoid protruding outward, narrower at tip.
/// - Outlined and labeled "C".
///
/// # Arguments
///
/// * `x` - Center x coordinate on screen.
/// * `y` - Center y coordinate on screen.
/// * `panel` - The `PanelInfo` to draw.
/// * `scale` - Scaling factor for sizes.
/// * `all_panels` - Full list for finding parent for controllers.
/// * `transformed_positions` - Precomputed screen positions of all panels.
fn draw_panel(
    x: f32,
    y: f32,
    panel: &PanelInfo,
    scale: f32,
    all_panels: &[PanelInfo],
    transformed_positions: &[(f32, f32)],
) {
    // Branch for controllers: panels with side_length <1.0 are non-light controllers,
    // visualized as yellow trapezoids attached to the nearest light panel's edge for realism.
    if panel.shape_type.side_length < 1.0 {
        // Select parent light panel: minimum distance to any valid (light) panel center.
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

        // Determine angular direction (atan2) from parent to controller for aligning with parent edges.
        let dx = x - parent_x;
        let dy = y - parent_y;
        let angle_to_controller = dy.atan2(dx);

        // Extract parent's num_sides and scaled side_length for radius and vertex computation.
        let num_sides = parent_panel.shape_type.num_sides();
        let parent_side_length = parent_panel.shape_type.side_length * scale;

        // Compute distance from center to vertex (circumradius) based on shape:
        // triangle: side / sqrt(3), square: side / sqrt(2), default: side.
        let parent_radius = if num_sides == 3 {
            parent_side_length / f32::sqrt(3.0)
        } else if num_sides == 4 {
            parent_side_length / f32::sqrt(2.0)
        } else {
            parent_side_length
        };

        // Select closest parent edge: iterate over vertex angles (adjusted by parent orientation),
        // find minimum angular difference to controller direction using shortest arc distance modulo 2π.
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

        // Position the edge endpoints: v1 and v2 at angles from parent orientation + edge index * angle_per_side.
        let v1_angle = parent_orientation + (closest_edge as f32 * angle_per_side);
        let v2_angle = parent_orientation + ((closest_edge + 1) as f32 * angle_per_side);

        let v1_x = parent_x + parent_radius * v1_angle.cos();
        let v1_y = parent_y + parent_radius * v1_angle.sin();
        let v2_x = parent_x + parent_radius * v2_angle.cos();
        let v2_y = parent_y + parent_radius * v2_angle.sin();

        // Define trapezoid vertices: top matches edge v1-v2, bottom parallel but shorter and offset
        // perpendicular outward by fixed height; fill with yellow triangles, outline in darker yellow.
        let trapezoid_height = 20.0;

        // Derive outward normal: vector from center to edge midpoint, normalized for extension.
        let edge_mid_x = (v1_x + v2_x) / 2.0;
        let edge_mid_y = (v1_y + v2_y) / 2.0;
        let perp_dx = edge_mid_x - parent_x;
        let perp_dy = edge_mid_y - parent_y;
        let perp_len = (perp_dx * perp_dx + perp_dy * perp_dy).sqrt();
        let perp_norm_x = perp_dx / perp_len;
        let perp_norm_y = perp_dy / perp_len;

        // Assemble trapezoid vertices array:
        // - Top: parent edge endpoints v1, v2
        // - Bottom: inset towards midpoint by (1-0.6=0.4), extended along perpendicular normal by height=20px
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

        // Render filled trapezoid by splitting into two triangles with solid yellow color.
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

        // Outline trapezoid edges with 2px thick darker yellow lines connecting vertices cyclically.
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

        // Add 'C' identifier: measure text, center at trapezoid centroid in black 10pt font.
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

    // Light panel rendering: compute polygon sides and scaled side length.
    let num_sides = panel.shape_type.num_sides();
    let side_length = panel.shape_type.side_length * scale;

    // Calculate circumradius (center to vertex) using geometry formulas per shape type.
    let radius = if num_sides == 3 {
        side_length / f32::sqrt(3.0)
    } else if num_sides == 4 {
        side_length / f32::sqrt(2.0)
    } else {
        side_length
    };

    // Compute vertex positions: for each side, angle = orientation_rad + i * (2π / n), offset from center by radius.
    let start_angle = (panel.orientation as f32).to_radians();
    let mut vertices = Vec::new();

    for i in 0..num_sides {
        let angle = start_angle + (i as f32 * 2.0 * PI / num_sides as f32);
        let vx = x + radius * angle.cos();
        let vy = y + radius * angle.sin();
        vertices.push(Vec2::new(vx, vy));
    }

    // Select panel fill color by shape ID groups for visual distinction (alpha=200 for transparency).
    let color = match panel.shape_type.id {
        0 | 8 | 9 => Color::from_rgba(255, 100, 100, 200),
        2..=4 => Color::from_rgba(100, 255, 100, 200),
        7 | 14 | 15 => Color::from_rgba(100, 150, 255, 200),
        30..=32 => Color::from_rgba(255, 255, 100, 200),
        _ => Color::from_rgba(150, 150, 150, 200),
    };

    // Fill the convex polygon using triangle fan: vertex[0] to consecutive pairs.
    for i in 1..(num_sides - 1) {
        draw_triangle(vertices[0], vertices[i], vertices[i + 1], color);
    }

    // Draw polygon boundary: connect consecutive vertices with white 2px lines.
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

    // Render panel ID text: format as string, measure for centering at panel center position.
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

/// Flashes a specific panel white briefly by sending UDP color updates.
///
/// Sets the clicked panel to white (Hwb(359,0,0)) and all other light panels to black (Hwb(0,0,1)).
/// Updates immediately (transition=1), waits 300ms, then sets all light panels back to black.
///
/// Only affects panels with side_length >=1.0 (light panels, skips controllers).
///
/// # Arguments
///
/// * `controller` - Initialized `NlUdp` instance for sending commands.
/// * `all_panels` - Full panel list to determine which to color.
/// * `clicked_panel_id` - ID of the panel to flash white.
fn flash_panel(
    controller: &crate::nanoleaf::NlUdp,
    all_panels: &[PanelInfo],
    clicked_panel_id: u16,
) {
    // Construct per-panel Hwb colors for UDP update: white (Hwb::new(359.0, 0.0, 0.0)) for clicked,
    // black (Hwb::new(0.0, 0.0, 1.0)) for other light panels; exclude controllers from array.
    let colors: Vec<Hwb> = all_panels
        .iter()
        .filter(|panel| panel.shape_type.side_length >= 1.0)
        .map(|panel| {
            if panel.panel_id == clicked_panel_id {
                Hwb::new(359.0, 0.0, 0.0) // White
            } else {
                Hwb::new(0.0, 0.0, 1.0) // Black
            }
        })
        .collect();

    // Send 'on' state: set clicked panel white, others black; immediate transition (duration=1).
    let _ = controller.update_panels(&colors, 1);

    // Sleep 300 milliseconds for visible flash duration.
    thread::sleep(Duration::from_millis(300));

    // Reset flash: update all light panels to black, effectively turning off the highlight
    // (note: this overrides any current effect; in practice, may need to restore original colors for seamless integration).
    let black_colors: Vec<Hwb> = all_panels
        .iter()
        .filter(|panel| panel.shape_type.side_length >= 1.0)
        .map(|_| Hwb::new(0.0, 0.0, 1.0))
        .collect();
    let _ = controller.update_panels(&black_colors, 1);
}
