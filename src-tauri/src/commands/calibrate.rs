//! 校准命令：由多张形态样本推导参考颜色与容差建议。

use tauri::State;

use crate::domain::calibrate::{
    suggest_feature_points, suggest_tolerances, FeatureCandidate, PointSuggestion,
};
use crate::domain::model::Region;
use crate::state::AppState;

/// 根据采集到的样本颜色计算每个点的参考颜色与容差建议。
#[tauri::command]
pub fn suggest_tolerances_command(
    _state: State<AppState>,
    samples: Vec<Vec<[u8; 4]>>,
    negative_samples: Vec<Vec<[u8; 4]>>,
    points_per_sample: usize,
    margin: u8,
) -> Result<Vec<PointSuggestion>, String> {
    suggest_tolerances(&samples, &negative_samples, points_per_sample, margin)
}

#[tauri::command]
pub fn suggest_feature_points_command(
    positive_pngs: Vec<String>,
    negative_pngs: Vec<String>,
    region: Region,
    limit: usize,
    minimum_distance: u32,
) -> Result<Vec<FeatureCandidate>, String> {
    if positive_pngs.len() > 200 || negative_pngs.len() > 200 {
        return Err("E3201: 正样本或负样本数量超过 200 张上限".to_string());
    }
    let positive = positive_pngs
        .iter()
        .map(|png| super::matching::decode_png(png))
        .collect::<Result<Vec<_>, _>>()?;
    let negative = negative_pngs
        .iter()
        .map(|png| super::matching::decode_png(png))
        .collect::<Result<Vec<_>, _>>()?;
    let positive_refs = positive
        .iter()
        .map(|(frame, width, height)| (frame.as_slice(), *width, *height))
        .collect::<Vec<_>>();
    let negative_refs = negative
        .iter()
        .map(|(frame, width, height)| (frame.as_slice(), *width, *height))
        .collect::<Vec<_>>();
    suggest_feature_points(
        &positive_refs,
        &negative_refs,
        region,
        limit,
        minimum_distance,
    )
    .map_err(|message| format!("E3202: {message}"))
}
