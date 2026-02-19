//! Rendu GPU du chrome navigateur (barre d'URL).
//!
//! Utilise `glow` pour les appels OpenGL et `fontdue` pour la rastérisation
//! CPU des glyphes. Les glyphes sont pré-rendus dans un atlas texture au
//! démarrage, puis dessinés comme des quads texturés à chaque frame.

use std::collections::HashMap;
use std::sync::Arc;

use glow::HasContext;

/// Hauteur du chrome en pixels physiques (default value, used by tests).
pub const CHROME_HEIGHT: u32 = 40;

const FONT_BYTES: &[u8] = include_bytes!("../resources/fonts/Inter-Regular.ttf");

/// Vertex shader GLES 300 es.
const VERTEX_SHADER: &str = r#"#version 300 es
precision mediump float;
layout(location = 0) in vec2 a_position;
layout(location = 1) in vec2 a_uv;
uniform mat4 u_projection;
out vec2 v_uv;
void main() {
    gl_Position = u_projection * vec4(a_position, 0.0, 1.0);
    v_uv = a_uv;
}
"#;

/// Fragment shader GLES 300 es.
const FRAGMENT_SHADER: &str = r#"#version 300 es
precision mediump float;
in vec2 v_uv;
uniform sampler2D u_texture;
uniform vec4 u_color;
uniform bool u_use_texture;
out vec4 fragColor;
void main() {
    if (u_use_texture) {
        float alpha = texture(u_texture, v_uv).r;
        fragColor = vec4(u_color.rgb, u_color.a * alpha);
    } else {
        fragColor = u_color;
    }
}
"#;

/// Informations par glyphe dans l'atlas.
struct GlyphInfo {
    /// Position X dans l'atlas (pixels).
    atlas_x: u32,
    /// Position Y dans l'atlas (pixels).
    atlas_y: u32,
    /// Largeur du glyphe (pixels).
    width: u32,
    /// Hauteur du glyphe (pixels).
    height: u32,
    /// Avance horizontale (pixels).
    advance_x: f32,
    /// Offset X depuis la position de base.
    offset_x: f32,
    /// Offset Y depuis la ligne de base (positif = vers le haut).
    offset_y: f32,
}

/// Atlas de glyphes pré-rendus.
struct GlyphAtlas {
    width: u32,
    height: u32,
    glyphs: HashMap<char, GlyphInfo>,
    pixels: Vec<u8>,
}

impl GlyphAtlas {
    fn build(font: &fontdue::Font, font_size: f32) -> Self {
        let chars: Vec<char> = (32u8..=126).map(|b| b as char).collect();

        // Premier passage : rastériser tous les glyphes pour calculer la taille
        let mut rasterized: Vec<(char, fontdue::Metrics, Vec<u8>)> = Vec::new();
        for &c in &chars {
            let (metrics, bitmap) = font.rasterize(c, font_size);
            rasterized.push((c, metrics, bitmap));
        }

        // Packing simple : rangées de gauche à droite
        let atlas_width: u32 = 512;
        let mut atlas_height: u32 = 64;
        let mut glyphs = HashMap::new();

        let mut x: u32 = 0;
        let mut y: u32 = 0;
        let mut row_height: u32 = 0;

        for &(c, ref metrics, _) in &rasterized {
            let w = metrics.width as u32;
            let h = metrics.height as u32;

            if x + w > atlas_width {
                x = 0;
                y += row_height + 1;
                row_height = 0;
            }

            if y + h > atlas_height {
                atlas_height = (atlas_height * 2).max(y + h + 1);
            }

            row_height = row_height.max(h);

            glyphs.insert(
                c,
                GlyphInfo {
                    atlas_x: x,
                    atlas_y: y,
                    width: w,
                    height: h,
                    advance_x: metrics.advance_width,
                    offset_x: metrics.xmin as f32,
                    offset_y: metrics.ymin as f32,
                },
            );

            x += w + 1;
        }

        atlas_height = (y + row_height + 1).next_power_of_two().max(64);

        // Remplir le buffer de pixels
        let mut pixels = vec![0u8; (atlas_width * atlas_height) as usize];
        for (c, _metrics, bitmap) in &rasterized {
            let info = &glyphs[c];
            for row in 0..info.height {
                for col in 0..info.width {
                    let src_idx = (row * info.width + col) as usize;
                    let dst_x = info.atlas_x + col;
                    let dst_y = info.atlas_y + row;
                    let dst_idx = (dst_y * atlas_width + dst_x) as usize;
                    if src_idx < bitmap.len() && dst_idx < pixels.len() {
                        pixels[dst_idx] = bitmap[src_idx];
                    }
                }
            }
        }

        Self {
            width: atlas_width,
            height: atlas_height,
            glyphs,
            pixels,
        }
    }
}

/// Renderer OpenGL pour le chrome du navigateur (barre d'URL).
pub struct ChromeRenderer {
    gl: Arc<glow::Context>,
    program: glow::Program,
    vao: glow::VertexArray,
    vbo: glow::Buffer,
    atlas_texture: glow::Texture,
    atlas: GlyphAtlas,
    u_projection: glow::UniformLocation,
    u_color: glow::UniformLocation,
    u_use_texture: glow::UniformLocation,
    u_texture: glow::UniformLocation,
    // Runtime theme values (from config)
    bg_color: [f32; 4],
    bg_focused_color: [f32; 4],
    text_color: [f32; 4],
    cursor_color: [f32; 4],
    bar_bg_color: [f32; 4],
    bar_border_color: [f32; 4],
    text_left_pad: f32,
    bar_margin: f32,
    bar_h_pad: f32,
    chrome_height: u32,
    font_size: f32,
}

#[allow(unsafe_op_in_unsafe_fn)]
impl ChromeRenderer {
    /// Crée le renderer. Doit être appelé avec un contexte GL actif.
    ///
    /// # Safety
    /// Appelle des fonctions OpenGL.
    pub unsafe fn new(gl: Arc<glow::Context>, config: &crate::config::ChromeConfig) -> Self {
        // ── Compiler les shaders ─────────────────────────────────────────
        let vs = gl.create_shader(glow::VERTEX_SHADER).unwrap();
        gl.shader_source(vs, VERTEX_SHADER);
        gl.compile_shader(vs);
        if !gl.get_shader_compile_status(vs) {
            panic!("Vertex shader error: {}", gl.get_shader_info_log(vs));
        }

        let fs = gl.create_shader(glow::FRAGMENT_SHADER).unwrap();
        gl.shader_source(fs, FRAGMENT_SHADER);
        gl.compile_shader(fs);
        if !gl.get_shader_compile_status(fs) {
            panic!("Fragment shader error: {}", gl.get_shader_info_log(fs));
        }

        let program = gl.create_program().unwrap();
        gl.attach_shader(program, vs);
        gl.attach_shader(program, fs);
        gl.link_program(program);
        if !gl.get_program_link_status(program) {
            panic!("Shader link error: {}", gl.get_program_info_log(program));
        }
        gl.delete_shader(vs);
        gl.delete_shader(fs);

        let u_projection = gl.get_uniform_location(program, "u_projection").unwrap();
        let u_color = gl.get_uniform_location(program, "u_color").unwrap();
        let u_use_texture = gl.get_uniform_location(program, "u_use_texture").unwrap();
        let u_texture = gl.get_uniform_location(program, "u_texture").unwrap();

        // ── VAO / VBO ────────────────────────────────────────────────────
        let vao = gl.create_vertex_array().unwrap();
        gl.bind_vertex_array(Some(vao));

        let vbo = gl.create_buffer().unwrap();
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));

        // Vertex layout: [x, y, u, v] x 6 vertices (2 triangles)
        let stride = 4 * std::mem::size_of::<f32>() as i32;
        // position (location = 0)
        gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, stride, 0);
        gl.enable_vertex_attrib_array(0);
        // uv (location = 1)
        gl.vertex_attrib_pointer_f32(
            1,
            2,
            glow::FLOAT,
            false,
            stride,
            2 * std::mem::size_of::<f32>() as i32,
        );
        gl.enable_vertex_attrib_array(1);

        gl.bind_vertex_array(None);

        // ── Atlas de glyphes ─────────────────────────────────────────────
        let font = fontdue::Font::from_bytes(FONT_BYTES, fontdue::FontSettings::default())
            .expect("Impossible de charger la police Inter");

        let atlas = GlyphAtlas::build(&font, config.font_size);

        let atlas_texture = gl.create_texture().unwrap();
        gl.bind_texture(glow::TEXTURE_2D, Some(atlas_texture));
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MIN_FILTER,
            glow::LINEAR as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MAG_FILTER,
            glow::LINEAR as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_WRAP_S,
            glow::CLAMP_TO_EDGE as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_WRAP_T,
            glow::CLAMP_TO_EDGE as i32,
        );
        gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
        gl.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            glow::R8 as i32,
            atlas.width as i32,
            atlas.height as i32,
            0,
            glow::RED,
            glow::UNSIGNED_BYTE,
            glow::PixelUnpackData::Slice(Some(&atlas.pixels)),
        );

        Self {
            gl,
            program,
            vao,
            vbo,
            atlas_texture,
            atlas,
            u_projection,
            u_color,
            u_use_texture,
            u_texture,
            bg_color: config.colors.background,
            bg_focused_color: config.colors.background_focused,
            text_color: config.colors.text,
            cursor_color: config.colors.cursor,
            bar_bg_color: config.colors.bar_background,
            bar_border_color: config.colors.bar_border,
            text_left_pad: config.text_left_pad,
            bar_margin: config.bar_margin,
            bar_h_pad: config.bar_h_pad,
            chrome_height: config.height,
            font_size: config.font_size,
        }
    }

    /// Dessine la barre d'URL.
    ///
    /// # Safety
    /// Appelle des fonctions OpenGL.
    pub unsafe fn draw(
        &self,
        window_width: u32,
        window_height: u32,
        url_text: &str,
        is_focused: bool,
        cursor_char_offset: Option<usize>,
    ) {
        let gl = &self.gl;
        let w = window_width as f32;
        let h = window_height as f32;
        let ch = self.chrome_height as f32;

        // ── Sauvegarder l'état GL ────────────────────────────────────────
        let prev_blend = gl.is_enabled(glow::BLEND);
        let prev_depth = gl.is_enabled(glow::DEPTH_TEST);
        let prev_scissor = gl.is_enabled(glow::SCISSOR_TEST);

        // ── Configurer l'état GL ─────────────────────────────────────────
        gl.viewport(0, 0, window_width as i32, window_height as i32);
        gl.disable(glow::DEPTH_TEST);
        gl.disable(glow::SCISSOR_TEST);
        gl.enable(glow::BLEND);
        gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);

        gl.use_program(Some(self.program));

        // Projection orthographique : (0,0) en haut-gauche, (w, h) en bas-droite.
        // OpenGL clip coords : X[-1,1] Y[-1,1]. On transforme :
        // x_clip = x * 2/w - 1
        // y_clip = 1 - y * 2/h  (inverser Y pour top-left origin)
        #[rustfmt::skip]
        let projection: [f32; 16] = [
            2.0 / w,  0.0,       0.0, 0.0,
            0.0,     -2.0 / h,   0.0, 0.0,
            0.0,      0.0,      -1.0, 0.0,
           -1.0,      1.0,       0.0, 1.0,
        ];
        gl.uniform_matrix_4_f32_slice(Some(&self.u_projection), false, &projection);
        gl.uniform_1_i32(Some(&self.u_texture), 0);

        gl.bind_vertex_array(Some(self.vao));

        // ── 1. Fond du chrome ────────────────────────────────────────────
        let bg = if is_focused {
            self.bg_focused_color
        } else {
            self.bg_color
        };
        self.draw_rect(0.0, 0.0, w, ch, bg);

        // ── 2. Barre de saisie (input field) ─────────────────────────────
        let bar_x = self.bar_margin;
        let bar_y = self.bar_margin;
        let bar_w = w - self.bar_margin * 2.0;
        let bar_h = ch - self.bar_margin * 2.0;

        // Bordure
        self.draw_rect(bar_x, bar_y, bar_w, bar_h, self.bar_border_color);
        // Fond intérieur
        self.draw_rect(
            bar_x + 1.0,
            bar_y + 1.0,
            bar_w - 2.0,
            bar_h - 2.0,
            self.bar_bg_color,
        );

        // ── 3. Texte de l'URL ────────────────────────────────────────────
        let text_x = bar_x + self.bar_h_pad + self.text_left_pad;
        // Centrer verticalement : baseline ≈ milieu du chrome
        let text_baseline_y = ch / 2.0 + self.font_size / 3.0;

        gl.active_texture(glow::TEXTURE0);
        gl.bind_texture(glow::TEXTURE_2D, Some(self.atlas_texture));

        let mut pen_x = text_x;
        let max_text_x = bar_x + bar_w - self.bar_h_pad;
        let mut cursor_x: Option<f32> = None;

        // Si le curseur est au début
        if cursor_char_offset == Some(0) {
            cursor_x = Some(pen_x);
        }

        for (char_idx, c) in url_text.chars().enumerate() {
            if pen_x > max_text_x {
                break;
            }

            if let Some(glyph) = self.atlas.glyphs.get(&c) {
                if glyph.width > 0 && glyph.height > 0 {
                    let gx = pen_x + glyph.offset_x;
                    // offset_y from fontdue is the bottom edge relative to baseline
                    // We need to position from top-left
                    let gy = text_baseline_y - glyph.offset_y - glyph.height as f32;

                    self.draw_textured_rect(
                        gx,
                        gy,
                        glyph.width as f32,
                        glyph.height as f32,
                        glyph.atlas_x,
                        glyph.atlas_y,
                        glyph.width,
                        glyph.height,
                    );
                }
                pen_x += glyph.advance_x;
            } else {
                // Caractère non présent dans l'atlas — avancer d'un espace
                if let Some(space) = self.atlas.glyphs.get(&' ') {
                    pen_x += space.advance_x;
                } else {
                    pen_x += self.font_size * 0.5;
                }
            }

            // Vérifier si le curseur est après ce caractère
            if cursor_char_offset == Some(char_idx + 1) {
                cursor_x = Some(pen_x);
            }
        }

        // ── 4. Curseur (si focusé) ───────────────────────────────────────
        if is_focused && let Some(cx) = cursor_x {
            let cursor_h = self.font_size + 4.0;
            let cursor_y = (ch - cursor_h) / 2.0;
            self.draw_rect(cx, cursor_y, 2.0, cursor_h, self.cursor_color);
        }

        // ── Restaurer l'état GL ──────────────────────────────────────────
        gl.bind_vertex_array(None);
        gl.use_program(None);

        if prev_depth {
            gl.enable(glow::DEPTH_TEST);
        }
        if !prev_blend {
            gl.disable(glow::BLEND);
        }
        if prev_scissor {
            gl.enable(glow::SCISSOR_TEST);
        }
    }

    /// Dessine un rectangle de couleur unie.
    unsafe fn draw_rect(&self, x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) {
        let gl = &self.gl;
        gl.uniform_1_i32(Some(&self.u_use_texture), 0);
        gl.uniform_4_f32_slice(Some(&self.u_color), &color);

        #[rustfmt::skip]
        let vertices: [f32; 24] = [
            // triangle 1
            x,     y,     0.0, 0.0,
            x + w, y,     0.0, 0.0,
            x + w, y + h, 0.0, 0.0,
            // triangle 2
            x,     y,     0.0, 0.0,
            x + w, y + h, 0.0, 0.0,
            x,     y + h, 0.0, 0.0,
        ];

        gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo));
        gl.buffer_data_u8_slice(
            glow::ARRAY_BUFFER,
            bytemuck_cast_slice(&vertices),
            glow::DYNAMIC_DRAW,
        );
        gl.draw_arrays(glow::TRIANGLES, 0, 6);
    }

    /// Dessine un rectangle texturé depuis l'atlas de glyphes.
    #[allow(clippy::too_many_arguments)]
    unsafe fn draw_textured_rect(
        &self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        atlas_x: u32,
        atlas_y: u32,
        atlas_w: u32,
        atlas_h: u32,
    ) {
        let gl = &self.gl;
        gl.uniform_1_i32(Some(&self.u_use_texture), 1);
        gl.uniform_4_f32_slice(Some(&self.u_color), &self.text_color);

        let aw = self.atlas.width as f32;
        let ah = self.atlas.height as f32;
        let u0 = atlas_x as f32 / aw;
        let v0 = atlas_y as f32 / ah;
        let u1 = (atlas_x + atlas_w) as f32 / aw;
        let v1 = (atlas_y + atlas_h) as f32 / ah;

        #[rustfmt::skip]
        let vertices: [f32; 24] = [
            x,     y,     u0, v0,
            x + w, y,     u1, v0,
            x + w, y + h, u1, v1,
            x,     y,     u0, v0,
            x + w, y + h, u1, v1,
            x,     y + h, u0, v1,
        ];

        gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo));
        gl.buffer_data_u8_slice(
            glow::ARRAY_BUFFER,
            bytemuck_cast_slice(&vertices),
            glow::DYNAMIC_DRAW,
        );
        gl.draw_arrays(glow::TRIANGLES, 0, 6);
    }
}

/// Cast safe d'un slice `[f32]` vers `[u8]` pour l'upload GL.
fn bytemuck_cast_slice(data: &[f32]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(data.as_ptr() as *const u8, std::mem::size_of_val(data)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_test_atlas() -> GlyphAtlas {
        let font = fontdue::Font::from_bytes(FONT_BYTES, fontdue::FontSettings::default())
            .expect("Failed to load Inter font");
        GlyphAtlas::build(&font, 16.0)
    }

    #[test]
    fn test_atlas_contains_all_ascii_printable() {
        let atlas = build_test_atlas();
        for b in 32u8..=126 {
            let c = b as char;
            assert!(
                atlas.glyphs.contains_key(&c),
                "Atlas missing char '{}' ({})",
                c,
                b
            );
        }
    }

    #[test]
    fn test_atlas_width_is_512() {
        let atlas = build_test_atlas();
        assert_eq!(atlas.width, 512);
    }

    #[test]
    fn test_atlas_height_valid() {
        let atlas = build_test_atlas();
        assert!(atlas.height > 0);
        assert!(atlas.height >= 64, "Atlas height should be >= 64");
    }

    #[test]
    fn test_atlas_pixel_buffer_size() {
        let atlas = build_test_atlas();
        assert_eq!(atlas.pixels.len(), (atlas.width * atlas.height) as usize);
    }

    #[test]
    fn test_glyph_advance_positive() {
        let atlas = build_test_atlas();
        for (&c, glyph) in &atlas.glyphs {
            assert!(
                glyph.advance_x > 0.0,
                "Glyph '{}' has non-positive advance_x: {}",
                c,
                glyph.advance_x
            );
        }
    }

    #[test]
    fn test_glyphs_within_atlas_bounds() {
        let atlas = build_test_atlas();
        for (&c, glyph) in &atlas.glyphs {
            assert!(
                glyph.atlas_x + glyph.width <= atlas.width,
                "Glyph '{}' exceeds atlas width: {} + {} > {}",
                c,
                glyph.atlas_x,
                glyph.width,
                atlas.width
            );
            assert!(
                glyph.atlas_y + glyph.height <= atlas.height,
                "Glyph '{}' exceeds atlas height: {} + {} > {}",
                c,
                glyph.atlas_y,
                glyph.height,
                atlas.height
            );
        }
    }

    #[test]
    fn test_no_overlapping_glyphs() {
        let atlas = build_test_atlas();
        let glyphs: Vec<_> = atlas.glyphs.iter().collect();
        for i in 0..glyphs.len() {
            for j in (i + 1)..glyphs.len() {
                let (&c1, g1) = glyphs[i];
                let (&c2, g2) = glyphs[j];
                // Skip zero-size glyphs (like space)
                if g1.width == 0 || g1.height == 0 || g2.width == 0 || g2.height == 0 {
                    continue;
                }
                let overlap_x =
                    g1.atlas_x < g2.atlas_x + g2.width && g2.atlas_x < g1.atlas_x + g1.width;
                let overlap_y =
                    g1.atlas_y < g2.atlas_y + g2.height && g2.atlas_y < g1.atlas_y + g1.height;
                assert!(
                    !(overlap_x && overlap_y),
                    "Glyphs '{}' and '{}' overlap",
                    c1,
                    c2
                );
            }
        }
    }

    #[test]
    fn test_space_has_zero_dimensions() {
        let atlas = build_test_atlas();
        let space = atlas.glyphs.get(&' ').expect("Space glyph missing");
        assert_eq!(space.width, 0, "Space should have width 0");
        assert_eq!(space.height, 0, "Space should have height 0");
        assert!(space.advance_x > 0.0, "Space should have positive advance");
    }

    #[test]
    fn test_bytemuck_cast_slice_length() {
        let data: [f32; 2] = [1.0, 2.0];
        let bytes = bytemuck_cast_slice(&data);
        assert_eq!(bytes.len(), 8); // 2 * 4 bytes
    }

    #[test]
    fn test_chrome_height_is_40() {
        assert_eq!(CHROME_HEIGHT, 40);
    }
}
