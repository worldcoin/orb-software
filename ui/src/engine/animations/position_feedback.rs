use crate::engine::{Animation, AnimationState, RingFrame};
use orb_rgb::Argb;
use std::{any::Any, f64::consts::PI};

/// Real-time position feedback animation that lights LEDs based on user position
/// The animation creates an arc of LEDs pointing toward the user's x,y position  
/// with arc length determined by distance from optimal position (closer = larger arc)
/// 
/// Coordinate System (based on logged calibration data):
/// - X: ~375-379 (optimal ~377.5) - horizontal position
/// - Y: ~-10 to -18 (optimal ~-17.5) - vertical position  
/// - Z: ~80-82 (optimal ~81.0) - depth/distance from camera
pub struct PositionFeedback<const N: usize> {
    /// Current user position in pixel/physical coordinates
    position_x: f64,
    position_y: f64,
    position_z: f64,
    
    /// Optimal position (based on logged "good" position values)
    optimal_x: f64,         // ~377.5 (center of logged x values)
    optimal_y: f64,         // ~-17.5 (center of logged y values)  
    optimal_z: f64,         // ~81.0 (center of logged z values)
    
    /// Animation parameters
    color: Argb,
    max_arc_angle: f64,     // Maximum arc angle in radians (when at center)
    min_arc_angle: f64,     // Minimum arc angle in radians (when at edge)
    center_threshold: f64,  // Distance from center considered "centered" (in pixels)
    edge_threshold: f64,    // Distance from center considered "at edge" (in pixels)
    _z_tolerance: f64,      // Z-axis tolerance for good positioning (unused for now)
    
    /// Animation state
    pulse_phase: f64,       // Phase for pulsing effect
    pulse_frequency: f64,   // Pulsing frequency in Hz
    target_intensity: f64,  // Target LED intensity
    current_intensity: f64, // Current LED intensity (smoothed)
    frame_count: u32,       // Frame counter for debug logging
}

impl<const N: usize> PositionFeedback<N> {
    pub fn new(color: Argb) -> Self {
        tracing::info!("Creating position feedback animation with color: {:?}", color);
        Self {
            position_x: 300.0,  // Start at optimal position
            position_y: -20.0,
            position_z: 70.0,

            // Optimal position based on logged "good" values
            optimal_x: 280.0,   // Center moved left (was 300.0)
            optimal_y: -20.0,   // Center of y range -10 to -18
            optimal_z: 70.0,    // Center of z range 80-82
            
            color,
            max_arc_angle: PI * 0.95,     // 171 degrees when perfectly centered (almost full ring)
            min_arc_angle: PI * 0.08,     // 14 degrees when far off (narrow guidance)
            center_threshold: 10.0,       // Within 10 pixels considered "centered" 
            edge_threshold: 40.0,         // Beyond 40 pixels considered "far"
                _z_tolerance: 5.0,             // Within 5 units of optimal Z
            pulse_phase: 0.0,
            pulse_frequency: 1.5,         // 1.5 Hz pulse for more responsiveness
            target_intensity: 1.0,        // Start with full intensity for debugging
            current_intensity: 1.0,       // Start with full intensity for debugging
            frame_count: 0,               // Initialize frame counter
        }
    }

    /// Update the user position (x, y, z in pixel/physical coordinates)
    pub fn update_position(&mut self, x: f64, y: f64, z: f64) {
        self.position_x = x;
        self.position_y = y;
        self.position_z = z;
        
        // Calculate some key metrics for debugging
        let offset_x = x - self.optimal_x;
        let offset_y = y - self.optimal_y;
        let _xy_distance = (offset_x * offset_x + offset_y * offset_y).sqrt(); // For future use
        let _z_distance = (z - self.optimal_z).abs(); // For future use
        
        tracing::info!(
            "Position update - pos:({:.1},{:.1},{:.1}) optimal:({:.1},{:.1},{:.1}) offset_x:{:.1} offset_y:{:.1} guidance_angle:{:.1}° arc:{:.1}°", 
            x, y, z,
            self.optimal_x, self.optimal_y, self.optimal_z,
            offset_x, 
            offset_y,
            self.calculate_guidance_angle() * 180.0 / PI,
            self.calculate_arc_length() * 180.0 / PI
        );
    }

    /// Calculate the guidance angle - where LEDs should light to guide user toward optimal position
    fn calculate_guidance_angle(&self) -> f64 {
        // Calculate offset from optimal position
        let offset_x = self.position_x - self.optimal_x;
        let offset_y = self.position_y - self.optimal_y;
        
        // Debug logging to understand coordinate mapping
        if self.frame_count % 30 == 0 {  // Log every second
            tracing::info!(
                "Position: ({:.1}, {:.1}) | Optimal: ({:.1}, {:.1}) | Offset: ({:.1}, {:.1})",
                self.position_x, self.position_y, 
                self.optimal_x, self.optimal_y,
                offset_x, offset_y
            );
        }
        
        // Calculate the direction the user should move (opposite of their offset)
        // If user is too far right (+X), they should move left (-X) → light LEFT LEDs
        // If user is too far down (+Y), they should move up (-Y) → light UP LEDs
        // Since current behavior matches position, flip to show guidance direction
        let guidance_x = offset_x;   // Flip to show opposite guidance
        let guidance_y = offset_y;   // Flip to show opposite guidance
        
        // Convert guidance direction to LED ring angle
        // atan2 gives angle from positive X-axis (3 o'clock), but LED 0 is at top (12 o'clock)
        // So we need to rotate by -90° to align: LED 0 = 12 o'clock = 0°
        let angle = guidance_x.atan2(guidance_y) - PI / 2.0;
        
        // Normalize to [0, 2π] where 0 is top of ring (LED 0)
        let normalized_angle = if angle < 0.0 {
            angle + 2.0 * PI
        } else {
            angle
        };
        
        // Debug logging for angle calculation
        if self.frame_count % 30 == 0 {  // Log every second
            tracing::info!(
                "Guidance: ({:.1}, {:.1}) | Raw angle: {:.2} rad ({:.1}°) | Normalized: {:.2} rad ({:.1}°)",
                guidance_x, guidance_y,
                angle, angle.to_degrees(),
                normalized_angle, normalized_angle.to_degrees()
            );
        }
        
        normalized_angle
    }

    /// Calculate arc length based on how far off-center the user is
    /// Further from center = smaller, more precise arc for guidance
    fn calculate_arc_length(&self) -> f64 {
        // Calculate individual axis offsets
        let offset_x = (self.position_x - self.optimal_x).abs();
        let offset_y = (self.position_y - self.optimal_y).abs();
        
        // Use the maximum offset (most off-center direction) to determine arc size
        let max_offset = offset_x.max(offset_y);
        
        if max_offset <= self.center_threshold {
            // Very close to optimal - large arc (almost full ring)
            self.max_arc_angle
        } else if max_offset >= self.edge_threshold {
            // Far from optimal - small, precise arc for guidance
            self.min_arc_angle
        } else {
            // Interpolate: closer to center = larger arc, further = smaller arc
            let t = (max_offset - self.center_threshold) / (self.edge_threshold - self.center_threshold);
            self.max_arc_angle - t * (self.max_arc_angle - self.min_arc_angle)
        }
    }

    /// Calculate intensity based on how far off-center the user is
    fn calculate_intensity(&self) -> f64 {
        // Calculate offset magnitudes
        let offset_x = (self.position_x - self.optimal_x).abs();
        let offset_y = (self.position_y - self.optimal_y).abs();
        let max_offset = offset_x.max(offset_y);
        
        // Base intensity: further from center = brighter guidance
        let base_intensity = if max_offset <= self.center_threshold {
            0.3  // Low intensity when well-positioned
        } else if max_offset >= self.edge_threshold {
            1.0  // High intensity when far off
        } else {
            // Interpolate: further from center = brighter
            let t = (max_offset - self.center_threshold) / (self.edge_threshold - self.center_threshold);
            0.3 + t * 0.7  // Scale from 0.3 to 1.0
        };
        
        // Add pulsing effect - more pulsing when further off-center for attention
        let pulse_strength = (max_offset / self.edge_threshold).clamp(0.0, 1.0);
        let pulse_multiplier = (1.0 - pulse_strength * 0.4) + pulse_strength * 0.4 * (self.pulse_phase.sin() * 0.5 + 0.5);
        
        base_intensity * pulse_multiplier
    }

    /// Check if a given LED index should be lit based on guidance direction
    fn should_light_led(&self, led_index: usize) -> bool {
        let guidance_angle = self.calculate_guidance_angle();
        let arc_length = self.calculate_arc_length();
        let half_arc = arc_length * 0.5;
        
        // Calculate this LED's angle
        let led_angle = (led_index as f64 / N as f64) * 2.0 * PI;
        
        // Calculate angular distance from guidance angle
        let mut angle_diff = (led_angle - guidance_angle).abs();
        if angle_diff > PI {
            angle_diff = 2.0 * PI - angle_diff;
        }
        
        // Light LED if within the guidance arc
        angle_diff <= half_arc
    }
}

impl<const N: usize> Animation for PositionFeedback<N> {
    type Frame = RingFrame<N>;

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn animate(&mut self, frame: &mut RingFrame<N>, dt: f64, idle: bool) -> AnimationState {
        if idle {
            return AnimationState::Running;
        }
        
        // Add a simple log to verify animate is being called
        if self.frame_count == 0 {
            tracing::info!("PositionFeedback animate() called for first time!");
        }

        // Update pulse phase
        self.pulse_phase += dt * self.pulse_frequency * 2.0 * PI;
        self.pulse_phase %= 2.0 * PI;

        // Update target intensity
        self.target_intensity = self.calculate_intensity();
        
        // Smooth intensity transitions
        let smooth_factor = 5.0; // How fast intensity changes
        self.current_intensity += (self.target_intensity - self.current_intensity) * dt * smooth_factor;

        // Debug output every few frames
        self.frame_count += 1;
        if self.frame_count % 30 == 0 { // Every second at 30fps
            tracing::info!(
                "Animation frame - intensity: {:.2}, target: {:.2}, pos: ({:.1}, {:.1}, {:.1})",
                self.current_intensity, self.target_intensity,
                self.position_x, self.position_y, self.position_z
            );
        }

        // Calculate offsets from optimal position
        let offset_x = self.position_x - self.optimal_x;
        let offset_y = self.position_y - self.optimal_y;
        let max_offset = offset_x.abs().max(offset_y.abs());
        
        let mut leds_lit = 0;
        let mut max_brightness = 0.0f64;
        
        if max_offset <= self.center_threshold {
            // Very close to optimal position - light full ring with low intensity
            tracing::info!("CENTERED MODE - max_offset: {:.1}, intensity: {:.2}", max_offset, self.current_intensity);
            for led in frame.iter_mut() {
                *led = self.color * self.current_intensity;
                leds_lit += 1;
                max_brightness = max_brightness.max(self.current_intensity);
            }
        } else if max_offset <= self.edge_threshold * 2.0 {
            // Show directional guidance arc
            let guidance_angle = self.calculate_guidance_angle();
            let arc_length = self.calculate_arc_length();
            
            tracing::info!(
                "GUIDANCE MODE - offset_x: {:.1}, offset_y: {:.1}, guidance_angle: {:.1}°, arc: {:.1}°, intensity: {:.2}", 
                offset_x,
                offset_y,
                guidance_angle * 180.0 / PI,
                arc_length * 180.0 / PI,
                self.current_intensity
            );
            
            // Debug: Show which LEDs should be lit for orientation testing
            let mut lit_leds = Vec::new();
            for i in 0..N {
                if self.should_light_led(i) {
                    lit_leds.push(i);
                }
            }
            if self.frame_count % 30 == 0 && !lit_leds.is_empty() {
                let led_count = lit_leds.len();
                let display_leds = if led_count <= 10 { 
                    format!("{:?}", lit_leds)
                } else { 
                    format!("{:?}..{:?}", &lit_leds[0..5], &lit_leds[led_count-5..])
                };
                tracing::info!("LEDs to light: {} (total: {})", display_leds, led_count);
            }
            
            for (i, led) in frame.iter_mut().enumerate() {
                if self.should_light_led(i) {
                    *led = self.color * self.current_intensity;
                    leds_lit += 1;
                    max_brightness = max_brightness.max(self.current_intensity);
                } else {
                    *led = Argb::OFF;
                }
            }
        } else {
            // Very far from optimal position - turn off all LEDs (user out of frame)
            tracing::info!("OUT OF FRAME - max_offset: {:.1}, turning off all LEDs", max_offset);
            for led in frame.iter_mut() {
                *led = Argb::OFF;
            }
        }

        if self.frame_count % 30 == 0 {
            tracing::info!(
                "LED Result - {} LEDs lit, max brightness: {:.2}, color: {:?}",
                leds_lit, max_brightness, self.color
            );
        }

        AnimationState::Running
    }

    fn stop(&mut self, _transition: crate::engine::Transition) -> eyre::Result<()> {
        Ok(())
    }
}
