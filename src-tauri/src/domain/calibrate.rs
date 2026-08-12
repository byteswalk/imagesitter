//! 校准统计：由同一对象多张形态样本自动推导每个采样点的参考颜色与容差。
//!
//! 校准流程：用户在目标对象处于不同形态时分别采集一组样本
//! （`samples[样本序号][点序号] = RGBA`），本模块对每个点逐通道取
//! min/max，中心值作为参考颜色，半区间加安全边距作为容差。

use serde::{Deserialize, Serialize};

use super::model::AlphaMode;

/// 单个采样点的校准建议。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PointSuggestion {
    /// 点序号。
    pub index: usize,
    /// 建议参考颜色 RGBA。
    pub reference: [u8; 4],
    /// 建议 RGB 容差。
    pub tolerance: [u8; 3],
    /// 各通道观测到的最小值 RGBA。
    pub min_observed: [u8; 4],
    /// 各通道观测到的最大值 RGBA。
    pub max_observed: [u8; 4],
    /// alpha 在正样本内是否稳定（波动未超过自适应阈值）。
    pub alpha_stable: bool,
    /// alpha 是否始终接近完全不透明；此时参与比较通常没有区分价值。
    pub alpha_opaque: bool,
    /// 建议采用的 alpha 匹配方式。
    pub suggested_alpha_mode: AlphaMode,
    /// 建议 alpha 容差；alpha 不稳定时使用。
    pub alpha_tolerance: u8,
    /// 正样本中观测到的 alpha 极差。
    pub alpha_range: u8,
    /// 使用当前建议时会误命中的负样本数量。
    pub negative_matches: usize,
    /// 颜色抖动幅度（RGB 最大半区间），用于提示该点是否适合做特征点。
    pub max_half_range: u8,
    /// 综合正样本稳定性与负样本区分度的质量分（0～100）。
    pub quality_score: u8,
    /// 是否建议保留该点。
    pub recommend_keep: bool,
    /// 质量结论。
    pub quality_reason: String,
}

/// 从完整正负样本区域中自动发现的候选特征点。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeatureCandidate {
    pub dx: u32,
    pub dy: u32,
    pub reference: [u8; 4],
    pub tolerance: [u8; 3],
    pub quality_score: u8,
    pub positive_range: u8,
    pub negative_distance: u8,
}

/// 综合正样本稳定性、负样本区分度和局部边缘强度选择稀疏特征点。
pub fn suggest_feature_points(
    positive_frames: &[(&[u8], u32, u32)],
    negative_frames: &[(&[u8], u32, u32)],
    region: super::model::Region,
    limit: usize,
    minimum_distance: u32,
) -> Result<Vec<FeatureCandidate>, String> {
    if positive_frames.len() < 2 {
        return Err("多样本智能选点至少需要 2 张正样本".to_string());
    }
    if negative_frames.is_empty() {
        return Err("多样本智能选点至少需要 1 张负样本".to_string());
    }
    let first_size = (positive_frames[0].1, positive_frames[0].2);
    for (_, width, height) in positive_frames.iter().chain(negative_frames.iter()) {
        if (*width, *height) != first_size {
            return Err("智能选点样本尺寸不一致".to_string());
        }
    }
    let (width, height) = first_size;
    if region.w == 0
        || region.h == 0
        || region
            .x
            .checked_add(region.w)
            .is_none_or(|right| right > width)
        || region
            .y
            .checked_add(region.h)
            .is_none_or(|bottom| bottom > height)
    {
        return Err("智能选点区域超出样本帧".to_string());
    }
    let pixels = u64::from(region.w) * u64::from(region.h);
    let step = ((pixels as f64 / 60_000.0).sqrt().ceil() as u32).max(1);
    let mut scored = Vec::new();
    for dy in (1..region.h.saturating_sub(1)).step_by(step as usize) {
        for dx in (1..region.w.saturating_sub(1)).step_by(step as usize) {
            let x = region.x + dx;
            let y = region.y + dy;
            let mut min = [255u8; 4];
            let mut max = [0u8; 4];
            let mut sums = [0u32; 4];
            for (frame, _, _) in positive_frames {
                let pixel = pixel_at(frame, width, x, y);
                for channel in 0..4 {
                    min[channel] = min[channel].min(pixel[channel]);
                    max[channel] = max[channel].max(pixel[channel]);
                    sums[channel] += u32::from(pixel[channel]);
                }
            }
            let count = positive_frames.len() as u32;
            let reference = [
                (sums[0] / count) as u8,
                (sums[1] / count) as u8,
                (sums[2] / count) as u8,
                (sums[3] / count) as u8,
            ];
            let positive_range = (0..3)
                .map(|channel| max[channel] - min[channel])
                .max()
                .unwrap_or(0);
            let negative_distance = negative_frames
                .iter()
                .map(|(frame, _, _)| {
                    let pixel = pixel_at(frame, width, x, y);
                    ((u16::from(reference[0].abs_diff(pixel[0]))
                        + u16::from(reference[1].abs_diff(pixel[1]))
                        + u16::from(reference[2].abs_diff(pixel[2])))
                        / 3) as u8
                })
                .min()
                .unwrap_or(0);
            let first = positive_frames[0].0;
            let center = pixel_at(first, width, x, y);
            let edge = [
                pixel_at(first, width, x - 1, y),
                pixel_at(first, width, x + 1, y),
                pixel_at(first, width, x, y - 1),
                pixel_at(first, width, x, y + 1),
            ]
            .iter()
            .map(|pixel| {
                (u16::from(center[0].abs_diff(pixel[0]))
                    + u16::from(center[1].abs_diff(pixel[1]))
                    + u16::from(center[2].abs_diff(pixel[2])))
                    / 3
            })
            .max()
            .unwrap_or(0) as u8;
            let stability = 100u16.saturating_sub(u16::from(positive_range).saturating_mul(2));
            let separation = (u16::from(negative_distance).saturating_mul(2)).min(100);
            let edge_score = u16::from(edge).min(100);
            let quality_score = ((stability * 45 + separation * 45 + edge_score * 10) / 100) as u8;
            if quality_score < 45 || negative_distance <= positive_range.saturating_add(4) {
                continue;
            }
            let tolerance = [
                (max[0] - min[0]).div_ceil(2).saturating_add(8),
                (max[1] - min[1]).div_ceil(2).saturating_add(8),
                (max[2] - min[2]).div_ceil(2).saturating_add(8),
            ];
            scored.push(FeatureCandidate {
                dx,
                dy,
                reference,
                tolerance,
                quality_score,
                positive_range,
                negative_distance,
            });
        }
    }
    scored.sort_by_key(|candidate| std::cmp::Reverse(candidate.quality_score));
    let mut selected: Vec<FeatureCandidate> = Vec::new();
    let distance_squared = u64::from(minimum_distance.max(1)).pow(2);
    for candidate in scored {
        if selected.iter().any(|existing| {
            let dx = i64::from(existing.dx) - i64::from(candidate.dx);
            let dy = i64::from(existing.dy) - i64::from(candidate.dy);
            ((dx * dx + dy * dy) as u64) < distance_squared
        }) {
            continue;
        }
        selected.push(candidate);
        if selected.len() >= limit.clamp(1, 64) {
            break;
        }
    }
    Ok(selected)
}

fn pixel_at(frame: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let offset = ((u64::from(y) * u64::from(width) + u64::from(x)) * 4) as usize;
    [
        frame[offset],
        frame[offset + 1],
        frame[offset + 2],
        frame[offset + 3],
    ]
}

/// 对多张样本推导每个点的参考颜色与容差建议。
///
/// `samples[样本序号][点序号] = RGBA`；所有样本必须包含相同数量的点。
/// `margin` 是叠加在半区间上的安全边距，补偿光照与动画微抖。
pub fn suggest_tolerances(
    samples: &[Vec<[u8; 4]>],
    negative_samples: &[Vec<[u8; 4]>],
    points_per_sample: usize,
    margin: u8,
) -> Result<Vec<PointSuggestion>, String> {
    if samples.is_empty() {
        return Err("至少需要一张校准样本".to_string());
    }
    if points_per_sample == 0 {
        return Err("当前组没有配置采样点".to_string());
    }

    let mut suggestions = Vec::with_capacity(points_per_sample);
    for index in 0..points_per_sample {
        let mut min_observed = [255u8; 4];
        let mut max_observed = [0u8; 4];
        for (sample_index, sample) in samples.iter().enumerate() {
            if sample.len() != points_per_sample {
                return Err(format!(
                    "样本 {} 的点数量 {} 与配置的 {} 不一致",
                    sample_index + 1,
                    sample.len(),
                    points_per_sample
                ));
            }
            let pixel = sample[index];
            for channel in 0..4 {
                min_observed[channel] = min_observed[channel].min(pixel[channel]);
                max_observed[channel] = max_observed[channel].max(pixel[channel]);
            }
        }

        let mut reference = [0u8; 4];
        let mut tolerance = [0u8; 3];
        let mut max_half_range = 0u8;
        for channel in 0..4 {
            let center = ((min_observed[channel] as u16 + max_observed[channel] as u16) / 2) as u8;
            let half =
                (max_observed[channel] as u16 - min_observed[channel] as u16).div_ceil(2) as u8;
            reference[channel] = center;
            if channel < 3 {
                tolerance[channel] = half.saturating_add(margin);
                max_half_range = max_half_range.max(half);
            }
        }
        let alpha_range = max_observed[3] - min_observed[3];
        let alpha_stability_limit = margin.saturating_mul(2).clamp(8, 32);
        let alpha_stable = alpha_range <= alpha_stability_limit;
        let alpha_opaque = min_observed[3] >= 250;
        let suggested_alpha_mode = if alpha_stable && !alpha_opaque {
            AlphaMode::Match
        } else {
            AlphaMode::Ignore
        };
        let alpha_half = u16::from(alpha_range).div_ceil(2) as u8;
        let alpha_tolerance = alpha_half.saturating_add(margin);
        let mut negative_matches = 0usize;
        for (sample_index, sample) in negative_samples.iter().enumerate() {
            if sample.len() != points_per_sample {
                return Err(format!(
                    "负样本 {} 的点数量 {} 与配置的 {} 不一致",
                    sample_index + 1,
                    sample.len(),
                    points_per_sample
                ));
            }
            let pixel = sample[index];
            let rgb_matches = (0..3)
                .all(|channel| reference[channel].abs_diff(pixel[channel]) <= tolerance[channel]);
            let alpha_matches = suggested_alpha_mode == AlphaMode::Ignore
                || reference[3].abs_diff(pixel[3]) <= alpha_tolerance;
            if rgb_matches && alpha_matches {
                negative_matches += 1;
            }
        }
        let stability_penalty = u16::from(max_half_range).saturating_mul(2).min(55);
        let collision_penalty = (negative_matches as u16).saturating_mul(45).min(80);
        let evidence_penalty = if negative_samples.is_empty() { 15 } else { 0 };
        let quality_score = 100u16
            .saturating_sub(stability_penalty)
            .saturating_sub(collision_penalty)
            .saturating_sub(evidence_penalty) as u8;
        let recommend_keep = quality_score >= 60 && negative_matches == 0 && max_half_range <= 30;
        let quality_reason = if negative_matches > 0 {
            format!("命中 {negative_matches} 张负样本，区分度不足")
        } else if max_half_range > 30 {
            "正样本颜色波动过大".to_string()
        } else if negative_samples.is_empty() {
            "尚无负样本，评分已降低".to_string()
        } else {
            "正样本稳定且能区分负样本".to_string()
        };
        suggestions.push(PointSuggestion {
            index,
            reference,
            tolerance,
            min_observed,
            max_observed,
            alpha_stable,
            alpha_opaque,
            suggested_alpha_mode,
            alpha_tolerance,
            alpha_range,
            negative_matches,
            max_half_range,
            quality_score,
            recommend_keep,
            quality_reason,
        });
    }
    Ok(suggestions)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centers_and_expands_tolerance() {
        let samples = vec![
            vec![[100, 100, 100, 255], [0, 0, 0, 255]],
            vec![[120, 80, 100, 255], [10, 10, 10, 255]],
        ];
        let suggestions = suggest_tolerances(&samples, &[], 2, 5).unwrap();
        let first = &suggestions[0];
        assert_eq!(first.reference[0], 110);
        assert_eq!(first.tolerance[0], 10 + 5);
        assert!(first.alpha_stable);
        assert_eq!(first.suggested_alpha_mode, AlphaMode::Ignore);
        assert_eq!(first.min_observed[1], 80);
        assert_eq!(first.max_observed[1], 100);
    }

    #[test]
    fn recommends_stable_transparency_and_reports_negative_collisions() {
        let positives = vec![vec![[100, 100, 100, 125]], vec![[104, 98, 102, 129]]];
        let negatives = vec![vec![[102, 100, 101, 127]], vec![[220, 20, 20, 255]]];
        let suggestion = &suggest_tolerances(&positives, &negatives, 1, 4).unwrap()[0];
        assert_eq!(suggestion.suggested_alpha_mode, AlphaMode::Match);
        assert_eq!(suggestion.negative_matches, 1);
        assert!(!suggestion.recommend_keep);
        assert!(suggestion.quality_score < 60);
    }

    #[test]
    fn feature_discovery_prefers_stable_positive_negative_difference() {
        let mut positive_a = vec![0u8; 5 * 5 * 4];
        let mut positive_b = vec![0u8; 5 * 5 * 4];
        let negative = vec![0u8; 5 * 5 * 4];
        let center = (2 * 5 + 2) * 4;
        positive_a[center..center + 4].copy_from_slice(&[220, 40, 20, 255]);
        positive_b[center..center + 4].copy_from_slice(&[218, 42, 22, 255]);
        let positives = vec![(positive_a.as_slice(), 5, 5), (positive_b.as_slice(), 5, 5)];
        let negatives = vec![(negative.as_slice(), 5, 5)];
        let candidates = suggest_feature_points(
            &positives,
            &negatives,
            super::super::model::Region {
                x: 0,
                y: 0,
                w: 5,
                h: 5,
            },
            8,
            1,
        )
        .unwrap();
        assert!(candidates
            .iter()
            .any(|candidate| candidate.dx == 2 && candidate.dy == 2));
        assert!(candidates
            .iter()
            .all(|candidate| candidate.negative_distance > candidate.positive_range));
    }
}
