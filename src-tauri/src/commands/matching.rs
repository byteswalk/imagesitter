//! 匹配命令：在目标窗口最新帧上运行特征组匹配。

use base64::Engine as _;
use tauri::State;

use crate::domain::engine::{evaluate_groups_search, validate_match_input, MatchReport};
use crate::domain::model::{FeatureGroup, Region};
use crate::state::AppState;

use super::grab_frame;

#[tauri::command]
pub fn run_match_advanced(
    state: State<AppState>,
    target_id: u64,
    region: Region,
    groups: Vec<FeatureGroup>,
    search_radius: u32,
    scale_search_percent: u32,
) -> Result<MatchReport, String> {
    let (frame, width, height) = grab_frame(&state, target_id)?;
    validate_match_input(&frame, width, height, &region, &groups)
        .map_err(|message| format!("E3001: {message}"))?;
    evaluate_groups_search(
        &frame,
        width,
        height,
        &region,
        &groups,
        search_radius,
        scale_search_percent,
    )
    .map_err(|message| format!("E3002: {message}"))
}

pub(crate) fn decode_png(png_data_url: &str) -> Result<(Vec<u8>, u32, u32), String> {
    const MAX_ENCODED_BYTES: usize = 24 * 1024 * 1024;
    if png_data_url.len() > MAX_ENCODED_BYTES {
        return Err("E3101: 回放图像超过 24 MiB 编码上限".to_string());
    }
    let encoded = png_data_url
        .strip_prefix("data:image/png;base64,")
        .unwrap_or(png_data_url);
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded.as_bytes())
        .map_err(|error| format!("E3102: 回放图像 Base64 无效：{error}"))?;
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err("E3103: 回放图像不是 PNG".to_string());
    }
    let image = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
        .map_err(|error| format!("E3104: 无法解码回放 PNG：{error}"))?
        .to_rgba8();
    let (width, height) = image.dimensions();
    if u64::from(width) * u64::from(height) > 100_000_000 {
        return Err("E3105: 回放图像像素数量超过安全上限".to_string());
    }
    Ok((image.into_raw(), width, height))
}

#[tauri::command]
pub fn run_match_png_advanced(
    png_data_url: String,
    region: Region,
    groups: Vec<FeatureGroup>,
    search_radius: u32,
    scale_search_percent: u32,
) -> Result<MatchReport, String> {
    let (frame, width, height) = decode_png(&png_data_url)?;
    validate_match_input(&frame, width, height, &region, &groups)
        .map_err(|message| format!("E3106: {message}"))?;
    evaluate_groups_search(
        &frame,
        width,
        height,
        &region,
        &groups,
        search_radius,
        scale_search_percent,
    )
    .map_err(|message| format!("E3107: {message}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::{AlphaMode, FeaturePoint};
    use std::io::Cursor;

    #[test]
    fn matches_an_embedded_png_without_a_live_target() {
        let image = image::RgbaImage::from_raw(1, 1, vec![10, 20, 30, 255]).unwrap();
        let mut png = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let data_url = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(png.into_inner())
        );
        let group = FeatureGroup {
            id: "normal".into(),
            name: "普通".into(),
            enabled: true,
            points: vec![FeaturePoint {
                dx: 0,
                dy: 0,
                reference: [10, 20, 30, 255],
                tolerance: [0, 0, 0],
                alpha_mode: AlphaMode::Ignore,
                alpha_tolerance: 0,
                must_not: false,
            }],
            min_match: -1,
        };
        let report = run_match_png_advanced(
            data_url,
            Region {
                x: 0,
                y: 0,
                w: 1,
                h: 1,
            },
            vec![group],
            0,
            0,
        )
        .unwrap();
        assert!(report.matched);
        assert_eq!(report.groups[0].points[0].similarity, 100);
    }
}
