//! 特征签名数据结构：与前端 TypeScript 类型和导出 JSON 规范保持一致。

use serde::{Deserialize, Serialize};

/// 窗口帧坐标系下的矩形区域；原点是捕获帧左上角。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Region {
    /// 区域左上角横坐标（像素）。
    pub x: u32,
    /// 区域左上角纵坐标（像素）。
    pub y: u32,
    /// 区域宽度（像素）。
    pub w: u32,
    /// 区域高度（像素）。
    pub h: u32,
}

/// alpha 通道参与匹配的方式。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AlphaMode {
    /// 忽略 alpha 通道：应对半透明蒙版、过渡色和显隐渐变。
    #[default]
    Ignore,
    /// alpha 也参与容差比对：需要区分透明背景与实体像素时使用。
    Match,
}

/// 特征组内的单个采样点；坐标相对区域左上角。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeaturePoint {
    /// 相对区域左上角的横向偏移。
    pub dx: u32,
    /// 相对区域左上角的纵向偏移。
    pub dy: u32,
    /// 参考颜色 RGBA。
    pub reference: [u8; 4],
    /// RGB 三通道各自的容差（0-255）。
    pub tolerance: [u8; 3],
    /// alpha 匹配方式。
    pub alpha_mode: AlphaMode,
    /// alpha 容差；仅 `Match` 模式生效。
    #[serde(default = "default_alpha_tolerance")]
    pub alpha_tolerance: u8,
    /// 排除点：该点必须不匹配才算通过，用于区分相似对象。
    #[serde(default)]
    pub must_not: bool,
}

fn default_alpha_tolerance() -> u8 {
    40
}

/// 同一对象的一种形态签名；组内所有必选点通过才算该组命中。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeatureGroup {
    /// 组唯一 ID。
    pub id: String,
    /// 组名称，例如"站立形态""受击形态"。
    pub name: String,
    /// 是否参与匹配；旧项目缺省为启用。
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// 组内采样点。
    pub points: Vec<FeaturePoint>,
    /// 最少需要通过的常规点数；-1 表示全部通过。
    #[serde(default = "default_min_match")]
    pub min_match: i32,
}

fn default_enabled() -> bool {
    true
}

fn default_min_match() -> i32 {
    -1
}
