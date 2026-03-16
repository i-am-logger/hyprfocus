use std::fs;
use std::path::PathBuf;

use crate::errors::{FocusError, Result};
use crate::theme::{self, Theme};

const SHADER_TEMPLATE: &str = r#"#version 300 es
precision highp float;

in vec2 v_texcoord;
uniform sampler2D tex;
out vec4 fragColor;

// Hyprfocus monochromatic shader
const vec3 themeColor = vec3({R}, {G}, {B});
const float intensity = {INTENSITY};
const float brightness = {BRIGHTNESS};

void main() {
    vec4 pixColor = texture(tex, v_texcoord);

    // Rec. 709 luminance
    float luminance = dot(pixColor.rgb, vec3({LUMA_R}, {LUMA_G}, {LUMA_B}));

    // Luminance mapped to theme color, scaled by brightness
    vec3 mono = luminance * themeColor * brightness;

    // Blend original and monochromatic
    vec3 result = mix(pixColor.rgb, mono, intensity);

    fragColor = vec4(result, pixColor.a);
}
"#;

/// Generate GLSL shader source for a theme with intensity, brightness, and saturation.
#[must_use]
pub fn generate_shader(theme: &Theme, intensity: f32, brightness: f32, saturation: f32) -> String {
    let color = theme.color.with_saturation(saturation);
    SHADER_TEMPLATE
        .replace("{R}", &format!("{:.4}", color.r))
        .replace("{G}", &format!("{:.4}", color.g))
        .replace("{B}", &format!("{:.4}", color.b))
        .replace("{INTENSITY}", &format!("{:.4}", intensity.clamp(0.0, 1.0)))
        .replace(
            "{BRIGHTNESS}",
            &format!("{:.4}", brightness.clamp(0.1, 2.0)),
        )
        .replace("{LUMA_R}", &format!("{:.4}", theme::LUMA_R))
        .replace("{LUMA_G}", &format!("{:.4}", theme::LUMA_G))
        .replace("{LUMA_B}", &format!("{:.4}", theme::LUMA_B))
}

/// Return the directory for shader files.
/// Prefers `$XDG_RUNTIME_DIR/focus/`, falls back to `/tmp/focus/`.
pub fn shader_dir() -> Result<PathBuf> {
    let base = std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"));

    if !base.exists() {
        return Err(FocusError::NoRuntimeDir);
    }

    Ok(base.join("hyprfocus"))
}

/// Write the shader to disk and return its path.
pub fn write_shader(
    theme: &Theme,
    intensity: f32,
    brightness: f32,
    saturation: f32,
) -> Result<PathBuf> {
    let dir = shader_dir()?;
    fs::create_dir_all(&dir).map_err(|e| FocusError::ShaderWriteFailed {
        path: dir.clone(),
        source: e,
    })?;

    let path = dir.join(format!(
        "hyprfocus-{}-i{:.0}-b{:.0}-s{:.0}.glsl",
        theme.name,
        intensity * 100.0,
        brightness * 100.0,
        saturation * 100.0
    ));
    let source = generate_shader(theme, intensity, brightness, saturation);

    fs::write(&path, source).map_err(|e| FocusError::ShaderWriteFailed {
        path: path.clone(),
        source: e,
    })?;

    log::info!("Wrote shader to {}", path.display());
    Ok(path)
}

/// Remove all focus shader files from the shader directory.
pub fn cleanup_shaders() -> Result<()> {
    let dir = shader_dir()?;
    if !dir.exists() {
        return Ok(());
    }

    let entries = fs::read_dir(&dir).map_err(|e| FocusError::ShaderRemoveFailed {
        path: dir.clone(),
        source: e,
    })?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("hyprfocus-") && n.ends_with(".glsl"))
        {
            fs::remove_file(&path).map_err(|e| FocusError::ShaderRemoveFailed {
                path: path.clone(),
                source: e,
            })?;
            log::debug!("Removed {}", path.display());
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{Color, Theme};
    use serial_test::serial;

    fn test_theme() -> Theme {
        Theme {
            name: "test",
            description: "test theme",
            color: Color::new(0.0, 1.0, 0.0),
            wavelength_range: (530, 560),
        }
    }

    fn amber_theme() -> Theme {
        Theme {
            name: "amber",
            description: "test",
            color: Color::new(1.0, 0.71, 0.0),
            wavelength_range: (598, 608),
        }
    }

    #[test]
    fn generate_contains_version() {
        let src = generate_shader(&test_theme(), 1.0, 1.0, 1.0);
        assert!(src.starts_with("#version 300 es"));
    }

    #[test]
    fn generate_contains_theme_color() {
        let src = generate_shader(&test_theme(), 1.0, 1.0, 1.0);
        assert!(src.contains("vec3(0.0000, 1.0000, 0.0000)"));
    }

    #[test]
    fn generate_contains_intensity() {
        let src = generate_shader(&test_theme(), 0.8, 1.0, 1.0);
        assert!(src.contains("const float intensity = 0.8000;"));
    }

    #[test]
    fn generate_full_intensity() {
        let src = generate_shader(&test_theme(), 1.0, 1.0, 1.0);
        assert!(src.contains("const float intensity = 1.0000;"));
    }

    #[test]
    fn generate_zero_intensity() {
        let src = generate_shader(&test_theme(), 0.0, 1.0, 1.0);
        assert!(src.contains("const float intensity = 0.0000;"));
    }

    #[test]
    fn generate_clamps_intensity() {
        let src = generate_shader(&test_theme(), 2.0, 1.0, 1.0);
        assert!(src.contains("const float intensity = 1.0000;"));

        let src = generate_shader(&test_theme(), -0.5, 1.0, 1.0);
        assert!(src.contains("const float intensity = 0.0000;"));
    }

    #[test]
    fn generate_has_valid_glsl_structure() {
        let src = generate_shader(&test_theme(), 1.0, 1.0, 1.0);
        assert!(src.contains("void main()"));
        assert!(src.contains("fragColor ="));
        assert!(src.contains("texture(tex, v_texcoord)"));
        assert!(src.contains("luminance"));
    }

    #[test]
    fn generate_brightness_default() {
        let src = generate_shader(&test_theme(), 1.0, 1.0, 1.0);
        assert!(src.contains("const float brightness = 1.0000;"));
    }

    #[test]
    fn generate_brightness_dim() {
        let src = generate_shader(&test_theme(), 1.0, 0.5, 1.0);
        assert!(src.contains("const float brightness = 0.5000;"));
    }

    #[test]
    fn generate_brightness_boost() {
        let src = generate_shader(&test_theme(), 1.0, 1.8, 1.0);
        assert!(src.contains("const float brightness = 1.8000;"));
    }

    #[test]
    fn generate_saturation_desaturate() {
        let src = generate_shader(&test_theme(), 1.0, 1.0, 0.0);
        assert!(src.contains("vec3(0.7152, 0.7152, 0.7152)"));
    }

    #[test]
    fn generate_saturation_boost() {
        let src = generate_shader(&test_theme(), 1.0, 1.0, 1.5);
        assert!(src.contains("vec3(0.0000, 1.0000, 0.0000)"));
    }

    #[test]
    fn generate_amber_color() {
        let src = generate_shader(&amber_theme(), 1.0, 1.0, 1.0);
        assert!(src.contains("vec3(1.0000, 0.7100, 0.0000)"));
    }

    #[test]
    #[serial]
    fn shader_dir_returns_path() {
        let dir = shader_dir().unwrap();
        assert!(dir.to_string_lossy().ends_with("/hyprfocus"));
    }

    #[test]
    #[serial]
    fn write_and_cleanup_shaders() {
        let original_xdg = std::env::var("XDG_RUNTIME_DIR").ok();
        let tmp = std::env::temp_dir().join("hyprfocus-test");
        std::fs::create_dir_all(&tmp).unwrap();
        unsafe { std::env::set_var("XDG_RUNTIME_DIR", &tmp) };

        // Write
        let path = write_shader(&test_theme(), 1.0, 1.0, 1.0).unwrap();
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("#version 300 es"));

        // Cleanup
        cleanup_shaders().unwrap();
        assert!(!path.exists());

        // Cleanup when no files exist is fine
        cleanup_shaders().unwrap();

        // Restore
        let _ = std::fs::remove_dir_all(&tmp);
        match original_xdg {
            Some(val) => unsafe { std::env::set_var("XDG_RUNTIME_DIR", val) },
            None => unsafe { std::env::remove_var("XDG_RUNTIME_DIR") },
        }
    }
}
