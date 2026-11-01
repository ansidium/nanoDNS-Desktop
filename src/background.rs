//! Per-fragment rendering of the final faded background field.
//!
//! Create and explicitly destroy this renderer on eframe's current GL thread.
//! Dropping it never calls GL: `App::on_exit` must call [`Renderer::destroy`].

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, ThreadId};

use eframe::egui;
use eframe::glow::{self, HasContext as _};

/// A small, context-bound renderer. Cloned paint callbacks share its resources.
pub struct Renderer {
    shared: Arc<Shared>,
}

struct Shared {
    owner: ThreadId,
    context_address: usize,
    resources: Mutex<Option<Resources>>,
    paints: AtomicU64,
}

struct Resources {
    program: glow::Program,
    vertices: glow::Buffer,
    vertex_array: Option<glow::VertexArray>,
    viewport: glow::UniformLocation,
    motion: glow::UniformLocation,
    depth_wave: glow::UniformLocation,
}

impl Renderer {
    /// Initializes once while the supplied eframe context is current.
    ///
    /// Desktop GL 2.0+ is supported, including contexts without vertex arrays.
    /// Shader/driver failures are returned to the caller for a marked fallback.
    pub fn new(gl: &glow::Context) -> Result<Self, String> {
        let version = gl.version();
        if version.is_embedded || version.major < 2 {
            return Err("background requires desktop OpenGL 2.0 or newer".into());
        }
        let (header, modern) = if version.major >= 3 {
            if version.major == 3 && version.minor == 0 {
                ("#version 130\n", true)
            } else {
                ("#version 140\n", true)
            }
        } else if version.minor >= 1 {
            ("#version 120\n", false)
        } else {
            ("#version 110\n", false)
        };
        let vertex_source = format!(
            "{header}{} vec2 a_position;\nvoid main() {{ gl_Position = vec4(a_position, 0.0, 1.0); }}\n",
            if modern { "in" } else { "attribute" }
        );
        let fragment_source = format!(
            "{header}{}\n{FRAGMENT_SHADER}",
            if modern {
                "out vec4 out_color;\n#define OUTPUT_COLOR out_color"
            } else {
                "#define OUTPUT_COLOR gl_FragColor"
            }
        );
        let program = create_program(gl, &vertex_source, &fragment_source)?;

        // SAFETY: initialization runs with the supplied context current. All
        // names below belong to it, and each failure deletes the names it owns.
        let resources = unsafe {
            let locations = (
                gl.get_uniform_location(program, "u_viewport"),
                gl.get_uniform_location(program, "u_motion"),
                gl.get_uniform_location(program, "u_depth_wave"),
            );
            let (Some(viewport), Some(motion), Some(depth_wave)) = locations else {
                gl.delete_program(program);
                return Err("background shader uniform was not found".into());
            };
            let vertices = match gl.create_buffer() {
                Ok(buffer) => buffer,
                Err(error) => {
                    gl.delete_program(program);
                    return Err(format!("background vertex buffer: {error}"));
                }
            };
            let extensions = gl.supported_extensions();
            let supports_vao = version.major >= 3
                || extensions.contains("GL_ARB_vertex_array_object")
                || extensions.contains("ARB_vertex_array_object");
            let vertex_array = if supports_vao {
                match gl.create_vertex_array() {
                    Ok(array) => Some(array),
                    Err(error) => {
                        gl.delete_buffer(vertices);
                        gl.delete_program(program);
                        return Err(format!("background vertex array: {error}"));
                    }
                }
            } else {
                None
            };
            // One oversized triangle covers the viewport without a diagonal
            // interpolation seam. Encoding the 24 bytes needs no slice cast.
            let positions = [-1.0_f32, -1.0, 3.0, -1.0, -1.0, 3.0];
            let mut bytes = [0_u8; 24];
            for (position, destination) in positions.iter().zip(bytes.as_chunks_mut::<4>().0) {
                destination.copy_from_slice(&position.to_ne_bytes());
            }
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vertices));
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, &bytes, glow::STATIC_DRAW);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
            Resources {
                program,
                vertices,
                vertex_array,
                viewport,
                motion,
                depth_wave,
            }
        };
        Ok(Self {
            shared: Arc::new(Shared {
                owner: thread::current().id(),
                context_address: std::ptr::from_ref(gl).addr(),
                resources: Mutex::new(Some(resources)),
                paints: AtomicU64::new(0),
            }),
        })
    }

    /// Paints the continuous field in `rect`; phase is the existing 26.3 s clock
    /// in radians. This does not request repaints or advance the clock.
    pub fn paint_callback(&self, rect: egui::Rect, phase: f32) -> egui::PaintCallback {
        let shared = Arc::clone(&self.shared);
        let phase = if phase.is_finite() {
            phase.rem_euclid(std::f32::consts::TAU)
        } else {
            0.0
        };
        // Uniform-only trigonometry is done once on the CPU, not per pixel.
        let angle = phase + 2.680_045;
        let (wave, cosine) = angle.sin_cos();
        let (cross, cross_cosine) = (2.0 * angle).sin_cos();
        let motion = [wave, cosine, cross, cross_cosine];
        let width_scale = 0.822_103_6 + 0.35 * (0.995_534_3 * wave + 0.094_400_67 * cosine);
        let depth_wave = 0.881_555_8 * (1.0 + 0.08 * (0.921_061 * wave - 0.389_418_33 * cosine))
            / width_scale.powf(0.25);
        egui::PaintCallback {
            rect,
            callback: Arc::new(eframe::egui_glow::CallbackFn::new(move |info, painter| {
                let gl = painter.gl();
                if shared.owner != thread::current().id()
                    || shared.context_address != std::ptr::from_ref(gl.as_ref()).addr()
                {
                    return;
                }
                let Ok(guard) = shared.resources.lock() else {
                    return;
                };
                let Some(resources) = guard.as_ref() else {
                    return;
                };
                let viewport = info.viewport_in_pixels();
                if viewport.width_px <= 0 || viewport.height_px <= 0 {
                    return;
                }
                // SAFETY: egui_glow invokes callbacks with this context current.
                // The owner/context checks and held lock keep every GL name live
                // through the draw. The VBO contains three complete vec2 values.
                unsafe {
                    gl.use_program(Some(resources.program));
                    if let Some(array) = resources.vertex_array {
                        gl.bind_vertex_array(Some(array));
                    }
                    gl.bind_buffer(glow::ARRAY_BUFFER, Some(resources.vertices));
                    gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, 8, 0);
                    gl.enable_vertex_attrib_array(0);
                    gl.uniform_4_f32(
                        Some(&resources.viewport),
                        viewport.left_px as f32,
                        viewport.from_bottom_px as f32,
                        viewport.width_px as f32,
                        viewport.height_px as f32,
                    );
                    gl.uniform_4_f32(
                        Some(&resources.motion),
                        motion[0],
                        motion[1],
                        motion[2],
                        motion[3],
                    );
                    gl.uniform_1_f32(Some(&resources.depth_wave), depth_wave);
                    // Own the quantization noise budget; restore this state
                    // because egui's prepare_painting does not manage DITHER.
                    let hardware_dither = gl.is_enabled(glow::DITHER);
                    if hardware_dither {
                        gl.disable(glow::DITHER);
                    }
                    gl.draw_arrays(glow::TRIANGLES, 0, 3);
                    shared.paints.fetch_add(1, Ordering::Relaxed);
                    if hardware_dither {
                        gl.enable(glow::DITHER);
                    }
                    if resources.vertex_array.is_some() {
                        gl.bind_vertex_array(None);
                    } else {
                        gl.disable_vertex_attrib_array(0);
                    }
                    gl.bind_buffer(glow::ARRAY_BUFFER, None);
                    // egui_glow restores its own program/VAO/attributes after
                    // each callback. No framebuffer or texture binding changed.
                }
            })),
        }
    }

    /// Number of callbacks that reached an actual draw on the owning context.
    pub fn paint_count(&self) -> u64 {
        self.shared.paints.load(Ordering::Relaxed)
    }

    /// Deletes resources once, on their original GL thread with that context
    /// current. Already queued callbacks become harmless no-ops after this.
    pub fn destroy(&self, gl: &glow::Context) -> Result<(), String> {
        if self.shared.owner != thread::current().id()
            || self.shared.context_address != std::ptr::from_ref(gl).addr()
        {
            return Err(
                "background destroy requires its original current GL context/thread".into(),
            );
        }
        // A poisoned lock still owns valid GL names; recover them for cleanup.
        let mut guard = self
            .shared
            .resources
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(resources) = guard.take() {
            // SAFETY: caller keeps the original context current on its owner
            // thread; the lock removes each owned name exactly once.
            unsafe {
                if let Some(array) = resources.vertex_array {
                    gl.delete_vertex_array(array);
                }
                gl.delete_buffer(resources.vertices);
                gl.delete_program(resources.program);
            }
        }
        Ok(())
    }
}

fn create_program(
    gl: &glow::Context,
    vertex_source: &str,
    fragment_source: &str,
) -> Result<glow::Program, String> {
    let vertex = compile_shader(gl, glow::VERTEX_SHADER, vertex_source)?;
    let fragment = match compile_shader(gl, glow::FRAGMENT_SHADER, fragment_source) {
        Ok(shader) => shader,
        Err(error) => {
            // SAFETY: caller holds the current context; vertex is owned here.
            unsafe { gl.delete_shader(vertex) };
            return Err(error);
        }
    };
    // SAFETY: caller holds the current context. Both shaders are live and owned
    // here; all paths delete them, and unsuccessful paths also delete program.
    unsafe {
        let program = match gl.create_program() {
            Ok(program) => program,
            Err(error) => {
                gl.delete_shader(vertex);
                gl.delete_shader(fragment);
                return Err(format!("background program creation: {error}"));
            }
        };
        gl.attach_shader(program, vertex);
        gl.attach_shader(program, fragment);
        gl.bind_attrib_location(program, 0, "a_position");
        gl.link_program(program);
        gl.detach_shader(program, vertex);
        gl.detach_shader(program, fragment);
        gl.delete_shader(vertex);
        gl.delete_shader(fragment);
        if gl.get_program_link_status(program) {
            Ok(program)
        } else {
            let error = gl.get_program_info_log(program);
            gl.delete_program(program);
            Err(format!("background shader link: {error}"))
        }
    }
}

fn compile_shader(
    gl: &glow::Context,
    shader_type: u32,
    source: &str,
) -> Result<glow::Shader, String> {
    // SAFETY: caller holds the current context, shader_type is a GL shader
    // constant, and the source is a valid Rust string for the duration of upload.
    unsafe {
        let shader = gl
            .create_shader(shader_type)
            .map_err(|error| format!("background shader creation: {error}"))?;
        gl.shader_source(shader, source);
        gl.compile_shader(shader);
        if gl.get_shader_compile_status(shader) {
            Ok(shader)
        } else {
            let error = gl.get_shader_info_log(shader);
            gl.delete_shader(shader);
            Err(format!("background shader compile: {error}"))
        }
    }
}

// This is the final faded B field from ui::ambient_row/ambient_color, evaluated
// in floating point per fragment instead of at quantized Color32 mesh vertices.
// egui_glow disables FRAMEBUFFER_SRGB before callbacks: encode exactly once here.
const FRAGMENT_SHADER: &str = r#"
uniform vec4 u_viewport;
uniform vec4 u_motion; // sin(theta), cos(theta), sin(2*theta), cos(2*theta)
uniform float u_depth_wave;

float square(float x) { return x * x; }

float envelope(float distance, float left_width, float right_width) {
    float z = distance / (distance < 0.0 ? left_width : right_width);
    float a = abs(z);
    return exp(-(distance < 0.0 ? a * a * a : z * z));
}

float srgb(float linear_value) {
    float c = max(linear_value, 0.0);
    if (c <= 0.0031308) { return 12.92 * c; }
    return 1.055 * pow(c, 1.0 / 2.4) - 0.055;
}

void main() {
    vec2 uv = (gl_FragCoord.xy - u_viewport.xy) / u_viewport.zw;
    float u = uv.x;
    float v = 1.0 - uv.y;
    float wave = u_motion.x;
    float cosine = u_motion.y;
    float cross_wave = u_motion.z;
    float cross_cosine = u_motion.w;
    float mix_value = 0.5 + 0.5 * (0.8525245 * wave + 0.52268726 * cosine);
    float balance = 0.6967067 * wave + 0.7173561 * cosine;
    float width_scale = 0.8221036 + 0.35 * (0.9955343 * wave + 0.09440067 * cosine);
    float axis = 0.14963824 + (0.1777767 + 0.003 * cross_wave) * v +
        0.0073640505 * (0.9527038 * wave + 0.30390042 * cosine);
    float left_width = (0.08804579 + 0.32705837 * v) * (1.0 + 0.07 * balance) * 0.8221036;
    float right_width = (0.09045617 + 0.33981422 * v) * (1.0 - 0.05 * balance) * width_scale;
    vec3 axes = vec3(
        0.16 + 0.12 * v + 0.03 * (0.62377465 * wave + 0.78160423 * cosine),
        0.21 + 0.36 * v + 0.03 * (-0.3206704 * wave + 0.9471908 * cosine),
        0.18 + 0.35 * v);
    vec3 widths = vec3(
        (0.012 + 0.018 * v) * (0.9 + 0.3 * mix_value) * 2.2638342,
        (0.025 + 0.035 * v) * (1.15 - 0.25 * mix_value) * 2.2638342,
        (0.06 + 0.12 * v) * (1.0 + 0.15 * cross_wave));
    float fade = exp(-square(max(v - 0.2, 0.0) / 0.24));
    vec3 strengths = fade * vec3(
        -0.3525872 * (0.14 + 0.70 * mix_value),
        -0.3525872 * (0.13 + 0.73 * (1.0 - mix_value)),
        0.4935457 * mix_value);
    float integral = 0.89298 * left_width + 0.886227 * right_width;
    float occupied =
        strengths.x * 1.772454 * widths.x * envelope(axes.x - axis, left_width, right_width) +
        strengths.y * 1.772454 * widths.y * envelope(axes.y - axis, left_width, right_width) +
        strengths.z * 1.772454 * widths.z * envelope(axes.z - axis, left_width, right_width);
    float gain = clamp(1.0 + 0.60 * (integral / max(integral + occupied, 0.03) - 1.0), 0.92, 1.32);
    float attenuation = pow(1.0 + 3.449008 * v, 1.8376188);
    float depth = u_depth_wave * gain / attenuation;
    vec3 fold_distance = (vec3(u) - axes) / widths;
    vec3 folds = exp(-fold_distance * fold_distance);
    float modulation = max(1.0 + dot(strengths, folds), 0.0);
    float fan = envelope(u - axis, left_width, right_width) * depth * modulation;
    fan += fade * 0.06292074 * exp(-square((u - (0.34 + 0.25 * v)) / (0.2 + 0.2 * v))) / attenuation;
    float curtain = 0.5 + 0.5 * (-0.09953952 * cross_wave + 0.9950336 * cross_cosine);
    fan += fade * 0.14999908 * curtain * exp(-square((u - (0.285 + 0.4 * v)) / (0.03 + 0.03 * v))) / attenuation;
    // Retain the original quiet lower reflection's phase and amplitude.
    float reflection_wave = -0.8953643 * wave - 0.44533452 * cosine;
    float reflection_cross = 0.6033543 * cross_wave + 0.79747325 * cross_cosine;
    float reflected = exp(-square((v - 0.8808129 - 0.002 * reflection_wave) / 0.10985217)) * (1.0 + 0.01 * reflection_cross);
    float reflected_distance = u - 0.1986849;
    float reflected_width = reflected_distance < 0.0 ? 0.3557304 : 0.23657057;
    float reflection = exp(-square(reflected_distance / reflected_width)) * reflected;
    float v2 = v * v;
    vec3 base = vec3(0.004391442, 0.005181517, 0.012286488) +
        vec3(0.003631751, 0.003952542, 0.003709805) * v2;
    vec3 linear_color = base +
        vec3(0.26845223, 0.19743945 * (1.0 + 0.5 * v2), 0.1698936 * (1.0 + v2)) * fan +
        vec3(0.044386417, 0.044445373, 0.043620296) * reflection;
    vec3 encoded = vec3(srgb(linear_color.r), srgb(linear_color.g), srgb(linear_color.b));
    // Stationary interleaved-gradient noise, shared by RGB to avoid colored
    // speckles. Its encoded-space amplitude is strictly below half an 8-bit LSB.
    vec2 pixel = floor(gl_FragCoord.xy);
    float noise = fract(52.9829189 * fract(dot(pixel, vec2(0.06711056, 0.00583715)))) - 0.5;
    encoded += vec3(noise * (0.95 / 255.0));
    OUTPUT_COLOR = vec4(clamp(encoded, 0.0, 1.0), 1.0);
}
"#;
