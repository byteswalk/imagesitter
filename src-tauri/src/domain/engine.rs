//! 特征匹配引擎：在 RGBA 帧上按区域和特征组逐点采样比对。
//!
//! 匹配语义：
//! - 组内常规点：实际颜色与参考颜色的各通道差值不超过容差即通过；
//! - 组内排除点（mustNot）：实际颜色与参考颜色**不**匹配才算通过；
//! - 组命中：通过的常规点数达到 minMatch（-1 表示全部）且所有排除点通过；
//! - 对象命中：任意一组命中。

use super::model::{AlphaMode, FeatureGroup, Region};
use serde::{Deserialize, Serialize};

/// 在进入匹配热路径前验证坐标、阈值与帧缓冲，避免无效项目产生静默误判。
pub fn validate_match_input(
    frame: &[u8],
    width: u32,
    height: u32,
    region: &Region,
    groups: &[FeatureGroup],
) -> Result<(), String> {
    let expected_len = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "帧尺寸计算溢出".to_string())?;
    if expected_len != frame.len() as u64 {
        return Err(format!(
            "帧缓冲长度 {} 与尺寸 {}x{} RGBA 不一致",
            frame.len(),
            width,
            height
        ));
    }
    if region.w == 0 || region.h == 0 {
        return Err("对象区域宽高必须大于 0".to_string());
    }
    let right = region
        .x
        .checked_add(region.w)
        .ok_or_else(|| "对象区域横坐标溢出".to_string())?;
    let bottom = region
        .y
        .checked_add(region.h)
        .ok_or_else(|| "对象区域纵坐标溢出".to_string())?;
    if right > width || bottom > height {
        return Err(format!(
            "对象区域 ({}, {}, {}, {}) 超出帧尺寸 {}x{}",
            region.x, region.y, region.w, region.h, width, height
        ));
    }
    for (group_index, group) in groups.iter().enumerate() {
        let regular_count = group.points.iter().filter(|point| !point.must_not).count();
        if group.min_match == 0 {
            return Err(format!("特征组 {} 的 minMatch 不能为 0", group_index + 1));
        }
        if group.min_match > 0 && group.min_match as usize > regular_count {
            return Err(format!(
                "特征组 {} 的 minMatch={} 超过常规点数量 {}",
                group_index + 1,
                group.min_match,
                regular_count
            ));
        }
        for (point_index, point) in group.points.iter().enumerate() {
            if point.dx >= region.w || point.dy >= region.h {
                return Err(format!(
                    "特征组 {} 的点 {} 坐标 ({}, {}) 超出对象区域 {}x{}",
                    group_index + 1,
                    point_index + 1,
                    point.dx,
                    point.dy,
                    region.w,
                    region.h
                ));
            }
        }
    }
    Ok(())
}

/// 单个采样点的比对结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PointResult {
    /// 点在组内的序号。
    pub index: usize,
    /// 是否通过。
    pub ok: bool,
    /// 越界或采样失败的原因说明；通过时为空。
    pub reason: String,
    /// 实际采样到的 RGBA；越界时为全零。
    pub actual: [u8; 4],
    /// 各通道绝对差值 RGBA；alpha 被忽略时为 0。
    pub delta: [u16; 4],
    /// 与参考色的直观相似度（0～100）。
    pub similarity: u8,
    /// 超过容差最严重的通道及超出量。
    pub max_excess: u16,
}

/// 单个特征组的匹配结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupResult {
    /// 组 ID。
    pub id: String,
    /// 组是否命中。
    pub matched: bool,
    /// 通过的常规点数。
    pub passed_count: usize,
    /// 要求的常规点数。
    pub required: usize,
    /// 每个点的明细。
    pub points: Vec<PointResult>,
}

/// 一次匹配的完整报告。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchReport {
    /// 对象是否命中（任意组命中）。
    pub matched: bool,
    /// 各组明细。
    pub groups: Vec<GroupResult>,
    /// 匹配耗时（微秒），用于评估实时性。
    pub elapsed_micros: u64,
    /// 相对适配后预期区域的最佳横向偏移。
    pub offset_x: i32,
    /// 相对适配后预期区域的最佳纵向偏移。
    pub offset_y: i32,
    /// 最佳尺寸倍率。
    pub matched_scale: f32,
}

/// 在整帧 RGBA 缓冲上评估多个特征组。
///
/// `frame` 为 `width * height * 4` 的 RGBA8 连续像素；`region` 指定对象区域。
pub fn evaluate_groups(
    frame: &[u8],
    width: u32,
    height: u32,
    region: &Region,
    groups: &[FeatureGroup],
) -> MatchReport {
    let started = std::time::Instant::now();
    let mut report_groups = Vec::with_capacity(groups.len());
    let mut any_matched = false;

    for group in groups {
        if !group.enabled {
            continue;
        }
        let mut points = Vec::with_capacity(group.points.len());
        let mut passed_required = 0usize;
        let mut required_total = 0usize;
        let mut exclusions_ok = true;

        for (index, point) in group.points.iter().enumerate() {
            let x = region.x.saturating_add(point.dx);
            let y = region.y.saturating_add(point.dy);
            let (ok, reason, actual, delta, similarity, max_excess) = if x >= width || y >= height {
                (
                    false,
                    "采样点超出捕获帧范围".to_string(),
                    [0, 0, 0, 0],
                    [0, 0, 0, 0],
                    0,
                    255,
                )
            } else {
                let offset = ((y * width + x) * 4) as usize;
                let actual = [
                    frame[offset],
                    frame[offset + 1],
                    frame[offset + 2],
                    frame[offset + 3],
                ];
                let delta = channel_delta(point.reference, actual, point.alpha_mode);
                let matched = within_tolerance(point.reference, actual, point, &delta);
                let channel_count = if point.alpha_mode == AlphaMode::Match {
                    4
                } else {
                    3
                };
                let difference: u32 = delta
                    .iter()
                    .take(channel_count)
                    .map(|value| u32::from(*value))
                    .sum();
                let similarity = 100u32
                    .saturating_sub(difference.saturating_mul(100) / (channel_count as u32 * 255))
                    as u8;
                let mut max_excess = 0u16;
                for (channel, value) in delta.iter().enumerate().take(3) {
                    max_excess =
                        max_excess.max(value.saturating_sub(point.tolerance[channel] as u16));
                }
                if point.alpha_mode == AlphaMode::Match {
                    max_excess =
                        max_excess.max(delta[3].saturating_sub(point.alpha_tolerance as u16));
                }
                (
                    matched,
                    String::new(),
                    actual,
                    delta,
                    similarity,
                    max_excess,
                )
            };

            // 排除点要求“不匹配”才算通过
            let passed = if point.must_not { !ok } else { ok };
            if point.must_not {
                if !passed {
                    exclusions_ok = false;
                }
            } else {
                required_total += 1;
                if passed {
                    passed_required += 1;
                }
            }

            points.push(PointResult {
                index,
                ok: passed,
                reason: if point.must_not && !passed {
                    "排除点意外命中参考颜色".to_string()
                } else if !passed && reason.is_empty() && x < width && y < height {
                    describe_excess(point, &delta)
                } else {
                    reason
                },
                actual,
                delta,
                similarity,
                max_excess,
            });
        }

        let required = if group.min_match < 0 {
            required_total
        } else {
            (group.min_match as usize).min(required_total)
        };
        let matched = exclusions_ok && passed_required >= required && required_total > 0;
        if matched {
            any_matched = true;
        }
        report_groups.push(GroupResult {
            id: group.id.clone(),
            matched,
            passed_count: passed_required,
            required,
            points,
        });
    }

    MatchReport {
        matched: any_matched,
        groups: report_groups,
        elapsed_micros: started.elapsed().as_micros() as u64,
        offset_x: 0,
        offset_y: 0,
        matched_scale: 1.0,
    }
}

/// 在适配后的预期区域附近搜索位置和小幅尺寸变化，返回最接近的命中或最佳诊断候选。
pub fn evaluate_groups_search(
    frame: &[u8],
    width: u32,
    height: u32,
    region: &Region,
    groups: &[FeatureGroup],
    search_radius: u32,
    scale_search_percent: u32,
) -> Result<MatchReport, String> {
    let started = std::time::Instant::now();
    let radius = search_radius.min(32) as i32;
    let scale_limit = scale_search_percent.min(10) as i32;
    let mut scales = vec![100i32];
    for step in 1..=scale_limit {
        scales.push(100 - step);
        scales.push(100 + step);
    }
    let mut offsets = Vec::new();
    for distance in 0..=radius {
        for dy in -distance..=distance {
            let dx = distance - dy.abs();
            offsets.push((dx, dy));
            if dx != 0 {
                offsets.push((-dx, dy));
            }
        }
    }

    let mut best: Option<(u64, MatchReport)> = None;
    for scale_percent in scales {
        let scaled_w = ((u64::from(region.w) * scale_percent as u64 + 50) / 100).max(1) as u32;
        let scaled_h = ((u64::from(region.h) * scale_percent as u64 + 50) / 100).max(1) as u32;
        let scaled_groups = scale_groups(groups, scale_percent, scaled_w, scaled_h);
        for &(dx, dy) in &offsets {
            let x = i64::from(region.x) + i64::from(dx);
            let y = i64::from(region.y) + i64::from(dy);
            if x < 0 || y < 0 {
                continue;
            }
            let candidate = Region {
                x: x as u32,
                y: y as u32,
                w: scaled_w,
                h: scaled_h,
            };
            if candidate
                .x
                .checked_add(candidate.w)
                .is_none_or(|right| right > width)
                || candidate
                    .y
                    .checked_add(candidate.h)
                    .is_none_or(|bottom| bottom > height)
            {
                continue;
            }
            let mut report = evaluate_groups(frame, width, height, &candidate, &scaled_groups);
            report.offset_x = dx;
            report.offset_y = dy;
            report.matched_scale = scale_percent as f32 / 100.0;
            let score = report_score(&report);
            if report.matched {
                report.elapsed_micros = started.elapsed().as_micros() as u64;
                return Ok(report);
            }
            if best
                .as_ref()
                .is_none_or(|(best_score, _)| score > *best_score)
            {
                best = Some((score, report));
            }
        }
    }
    let mut report = best
        .map(|(_, report)| report)
        .ok_or_else(|| "搜索范围内没有有效候选区域".to_string())?;
    report.elapsed_micros = started.elapsed().as_micros() as u64;
    Ok(report)
}

fn scale_groups(
    groups: &[FeatureGroup],
    percent: i32,
    width: u32,
    height: u32,
) -> Vec<FeatureGroup> {
    groups
        .iter()
        .cloned()
        .map(|mut group| {
            for point in &mut group.points {
                point.dx = (((u64::from(point.dx) * percent as u64 + 50) / 100) as u32)
                    .min(width.saturating_sub(1));
                point.dy = (((u64::from(point.dy) * percent as u64 + 50) / 100) as u32)
                    .min(height.saturating_sub(1));
            }
            group
        })
        .collect()
}

fn report_score(report: &MatchReport) -> u64 {
    report.groups.iter().fold(0u64, |score, group| {
        score
            + group.passed_count as u64 * 10_000
            + group
                .points
                .iter()
                .map(|point| u64::from(point.similarity))
                .sum::<u64>()
    })
}

/// 把逐通道超差转换为可直接展示的诊断文本。
fn describe_excess(point: &super::model::FeaturePoint, delta: &[u16; 4]) -> String {
    let names = ["R", "G", "B", "A"];
    let mut failures = Vec::new();
    for (channel, value) in delta.iter().enumerate().take(3) {
        let excess = value.saturating_sub(point.tolerance[channel] as u16);
        if excess > 0 {
            failures.push(format!("{} 超差 +{}", names[channel], excess));
        }
    }
    if point.alpha_mode == AlphaMode::Match {
        let excess = delta[3].saturating_sub(point.alpha_tolerance as u16);
        if excess > 0 {
            failures.push(format!("A 超差 +{excess}"));
        }
    }
    if failures.is_empty() {
        "颜色差值超出容差".to_string()
    } else {
        failures.join("，")
    }
}

/// 计算参考颜色与实际颜色的各通道差值；alpha 被忽略时该通道差值为 0。
fn channel_delta(reference: [u8; 4], actual: [u8; 4], alpha_mode: AlphaMode) -> [u16; 4] {
    let mut delta = [0u16; 4];
    for channel in 0..3 {
        delta[channel] = reference[channel].abs_diff(actual[channel]) as u16;
    }
    if alpha_mode == AlphaMode::Match {
        delta[3] = reference[3].abs_diff(actual[3]) as u16;
    }
    delta
}

/// 判断差值是否全部落在容差范围内。
fn within_tolerance(
    _reference: [u8; 4],
    _actual: [u8; 4],
    point: &super::model::FeaturePoint,
    delta: &[u16; 4],
) -> bool {
    for (channel, value) in delta.iter().take(3).enumerate() {
        if *value > point.tolerance[channel] as u16 {
            return false;
        }
    }
    if point.alpha_mode == AlphaMode::Match && delta[3] > point.alpha_tolerance as u16 {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::{AlphaMode, FeaturePoint};

    fn point(dx: u32, dy: u32, reference: [u8; 4], tolerance: [u8; 3]) -> FeaturePoint {
        FeaturePoint {
            dx,
            dy,
            reference,
            tolerance,
            alpha_mode: AlphaMode::Ignore,
            alpha_tolerance: 40,
            must_not: false,
        }
    }

    #[test]
    fn matches_within_tolerance_and_rejects_outside() {
        // 2x1 帧：(100,50,25) 与 (10,10,10)
        let frame = vec![100, 50, 25, 255, 10, 10, 10, 255];
        let region = Region {
            x: 0,
            y: 0,
            w: 2,
            h: 1,
        };
        let group = FeatureGroup {
            id: "g1".into(),
            name: "形态一".into(),
            enabled: true,
            points: vec![point(0, 0, [105, 45, 30, 255], [8, 8, 8])],
            min_match: -1,
        };
        let report = evaluate_groups(&frame, 2, 1, &region, std::slice::from_ref(&group));
        assert!(report.matched);

        let far = FeatureGroup {
            points: vec![point(1, 0, [200, 200, 200, 255], [5, 5, 5])],
            ..group
        };
        let report = evaluate_groups(&frame, 2, 1, &region, &[far]);
        assert!(!report.matched);
    }

    #[test]
    fn exclusion_point_blocks_match() {
        let frame = vec![100, 50, 25, 255];
        let region = Region {
            x: 0,
            y: 0,
            w: 1,
            h: 1,
        };
        let mut good = point(0, 0, [100, 50, 25, 255], [0, 0, 0]);
        good.must_not = true;
        let group = FeatureGroup {
            id: "g1".into(),
            name: "排除".into(),
            enabled: true,
            points: vec![point(0, 0, [100, 50, 25, 255], [255, 255, 255]), good],
            min_match: -1,
        };
        let report = evaluate_groups(&frame, 1, 1, &region, &[group]);
        assert!(!report.matched);
    }

    #[test]
    fn alpha_ignored_by_default() {
        // 参考 alpha=255，实际 alpha=80（半透明蒙版），Ignore 模式仍应通过
        let frame = vec![100, 50, 25, 80];
        let region = Region {
            x: 0,
            y: 0,
            w: 1,
            h: 1,
        };
        let group = FeatureGroup {
            id: "g1".into(),
            name: "蒙版".into(),
            enabled: true,
            points: vec![point(0, 0, [100, 50, 25, 255], [0, 0, 0])],
            min_match: -1,
        };
        let report = evaluate_groups(&frame, 1, 1, &region, &[group]);
        assert!(report.matched);
    }

    #[test]
    fn rejects_points_outside_region_including_exclusions() {
        let frame = vec![100, 50, 25, 255];
        let region = Region {
            x: 0,
            y: 0,
            w: 1,
            h: 1,
        };
        let mut outside = point(1, 0, [100, 50, 25, 255], [0, 0, 0]);
        outside.must_not = true;
        let group = FeatureGroup {
            id: "g1".into(),
            name: "越界".into(),
            enabled: true,
            points: vec![outside],
            min_match: -1,
        };
        assert!(validate_match_input(&frame, 1, 1, &region, &[group]).is_err());
    }

    #[test]
    fn disabled_state_is_not_evaluated() {
        let frame = vec![100, 50, 25, 255];
        let region = Region {
            x: 0,
            y: 0,
            w: 1,
            h: 1,
        };
        let group = FeatureGroup {
            id: "disabled".into(),
            name: "停用状态".into(),
            enabled: false,
            points: vec![point(0, 0, [100, 50, 25, 255], [0, 0, 0])],
            min_match: -1,
        };
        let report = evaluate_groups(&frame, 1, 1, &region, &[group]);
        assert!(!report.matched);
        assert!(report.groups.is_empty());
    }

    #[test]
    fn nearby_search_finds_shifted_object_and_reports_offset() {
        let mut frame = vec![0u8; 5 * 4];
        frame[3 * 4..3 * 4 + 4].copy_from_slice(&[200, 100, 50, 255]);
        let region = Region {
            x: 1,
            y: 0,
            w: 1,
            h: 1,
        };
        let group = FeatureGroup {
            id: "shifted".into(),
            name: "shifted".into(),
            enabled: true,
            points: vec![point(0, 0, [200, 100, 50, 255], [0, 0, 0])],
            min_match: -1,
        };
        let report = evaluate_groups_search(&frame, 5, 1, &region, &[group], 2, 0).unwrap();
        assert!(report.matched);
        assert_eq!(report.offset_x, 2);
        assert_eq!(report.offset_y, 0);
        assert_eq!(report.matched_scale, 1.0);
    }
}
