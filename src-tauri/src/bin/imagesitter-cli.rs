//! ImageSitter headless validator and replay runner.
//! Commands:
//!   imagesitter-cli validate <project.json>
//!   imagesitter-cli match <project.json> <object-id-or-name> <frame.png>
//!   imagesitter-cli test <project.json>

use base64::Engine as _;
use imagesitter_lib::domain::engine::{evaluate_groups_search, validate_match_input, MatchReport};
use imagesitter_lib::domain::model::{FeatureGroup, Region};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectFile {
    version: u32,
    #[serde(default)]
    target: Target,
    objects: Vec<ObjectRule>,
    #[serde(default)]
    replay_cases: Vec<ReplayCase>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Target {
    #[serde(default)]
    frame_width: u32,
    #[serde(default)]
    frame_height: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ObjectRule {
    id: String,
    name: String,
    region: RegionWire,
    groups: Vec<FeatureGroup>,
    #[serde(default)]
    coordinate_mode: CoordinateMode,
    #[serde(default)]
    anchor_x: Anchor,
    #[serde(default)]
    anchor_y: Anchor,
    #[serde(default)]
    search_radius: u32,
    #[serde(default)]
    scale_search_percent: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegionWire {
    #[serde(default)]
    left: Option<u32>,
    #[serde(default)]
    top: Option<u32>,
    #[serde(default)]
    right: Option<u32>,
    #[serde(default)]
    bottom: Option<u32>,
    #[serde(default)]
    x: Option<u32>,
    #[serde(default)]
    y: Option<u32>,
    #[serde(default)]
    w: Option<u32>,
    #[serde(default)]
    h: Option<u32>,
}

impl RegionWire {
    fn to_region(&self) -> Result<Region, String> {
        if let (Some(left), Some(top), Some(right), Some(bottom)) =
            (self.left, self.top, self.right, self.bottom)
        {
            if right <= left || bottom <= top {
                return Err("region right/bottom must be greater than left/top".into());
            }
            return Ok(Region {
                x: left,
                y: top,
                w: right - left,
                h: bottom - top,
            });
        }
        let region = Region {
            x: self.x.ok_or("region.x is missing")?,
            y: self.y.ok_or("region.y is missing")?,
            w: self.w.ok_or("region.w is missing")?,
            h: self.h.ok_or("region.h is missing")?,
        };
        if region.w == 0 || region.h == 0 {
            return Err("region width and height must be positive".into());
        }
        Ok(region)
    }
}

#[derive(Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum CoordinateMode {
    #[default]
    Fixed,
    Scale,
    Anchor,
}

#[derive(Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum Anchor {
    #[default]
    Start,
    Center,
    End,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReplayCase {
    id: String,
    name: String,
    #[serde(default = "embedded")]
    storage: String,
    #[serde(default)]
    png_data_url: String,
    #[serde(default)]
    relative_path: String,
    #[serde(default)]
    sha256: String,
    #[serde(default)]
    expectations: Vec<Expectation>,
}

fn embedded() -> String {
    "embedded".into()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Expectation {
    object_id: String,
    expected_group_id: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MatchOutput<'a> {
    object_id: &'a str,
    object_name: &'a str,
    frame_width: u32,
    frame_height: u32,
    report: MatchReport,
}

fn load_project(path: &Path) -> Result<ProjectFile, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read project {}: {error}", path.display()))?;
    let project: ProjectFile =
        serde_json::from_str(&content).map_err(|error| format!("invalid project JSON: {error}"))?;
    if !(1..=4).contains(&project.version) {
        return Err(format!("unsupported project version {}", project.version));
    }
    if project.objects.iter().any(|object| object.id.is_empty()) {
        return Err("object id must not be empty".into());
    }
    Ok(project)
}

fn load_png_bytes(bytes: &[u8]) -> Result<(Vec<u8>, u32, u32), String> {
    if bytes.len() > 24 * 1024 * 1024 {
        return Err("PNG exceeds the 24 MiB safety limit".into());
    }
    let image = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
        .map_err(|error| format!("cannot decode PNG: {error}"))?
        .to_rgba8();
    let (width, height) = image.dimensions();
    if u64::from(width) * u64::from(height) > 100_000_000 {
        return Err("PNG exceeds the 100 megapixel safety limit".into());
    }
    Ok((image.into_raw(), width, height))
}

fn load_png_file(path: &Path) -> Result<(Vec<u8>, u32, u32), String> {
    let bytes = std::fs::read(path)
        .map_err(|error| format!("cannot read PNG {}: {error}", path.display()))?;
    load_png_bytes(&bytes)
}

fn load_replay_png(
    project_path: &Path,
    sample: &ReplayCase,
) -> Result<(Vec<u8>, u32, u32), String> {
    if sample.storage == "embedded" {
        let encoded = sample
            .png_data_url
            .strip_prefix("data:image/png;base64,")
            .ok_or("embedded sample is not a PNG data URL")?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|error| format!("invalid embedded PNG base64: {error}"))?;
        return load_png_bytes(&bytes);
    }
    if sample.storage != "external" {
        return Err(format!("unsupported sample storage {}", sample.storage));
    }
    let relative = Path::new(&sample.relative_path);
    if relative.is_absolute()
        || relative.components().any(|part| {
            matches!(
                part,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err("unsafe external sample path".into());
    }
    let target = project_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(relative);
    let bytes = std::fs::read(&target)
        .map_err(|error| format!("cannot read external sample {}: {error}", target.display()))?;
    if !sample.sha256.is_empty() {
        let actual = format!("{:x}", Sha256::digest(&bytes));
        if !actual.eq_ignore_ascii_case(&sample.sha256) {
            return Err(format!(
                "external sample hash mismatch: {}",
                target.display()
            ));
        }
    }
    load_png_bytes(&bytes)
}

fn anchor_shift(anchor: &Anchor, delta: i64) -> i64 {
    match anchor {
        Anchor::Start => 0,
        Anchor::Center => delta / 2,
        Anchor::End => delta,
    }
}

fn resolve_object(
    object: &ObjectRule,
    base_width: u32,
    base_height: u32,
    frame_width: u32,
    frame_height: u32,
) -> Result<(Region, Vec<FeatureGroup>), String> {
    let region = object.region.to_region()?;
    let base_width = if base_width == 0 {
        frame_width
    } else {
        base_width
    };
    let base_height = if base_height == 0 {
        frame_height
    } else {
        base_height
    };
    if object.coordinate_mode == CoordinateMode::Fixed {
        if base_width != frame_width || base_height != frame_height {
            return Err(format!(
                "fixed coordinates require {}x{}, got {}x{}",
                base_width, base_height, frame_width, frame_height
            ));
        }
        return Ok((region, object.groups.clone()));
    }
    if object.coordinate_mode == CoordinateMode::Anchor {
        let x = i64::from(region.x)
            + anchor_shift(
                &object.anchor_x,
                i64::from(frame_width) - i64::from(base_width),
            );
        let y = i64::from(region.y)
            + anchor_shift(
                &object.anchor_y,
                i64::from(frame_height) - i64::from(base_height),
            );
        if x < 0 || y < 0 {
            return Err("anchored region moved outside the frame".into());
        }
        return Ok((
            Region {
                x: x as u32,
                y: y as u32,
                ..region
            },
            object.groups.clone(),
        ));
    }
    let scale_x = f64::from(frame_width) / f64::from(base_width.max(1));
    let scale_y = f64::from(frame_height) / f64::from(base_height.max(1));
    let scaled = Region {
        x: (f64::from(region.x) * scale_x).round() as u32,
        y: (f64::from(region.y) * scale_y).round() as u32,
        w: (f64::from(region.w) * scale_x).round().max(1.0) as u32,
        h: (f64::from(region.h) * scale_y).round().max(1.0) as u32,
    };
    let mut groups = object.groups.clone();
    for group in &mut groups {
        for point in &mut group.points {
            point.dx = (f64::from(point.dx) * scale_x)
                .round()
                .min(f64::from(scaled.w.saturating_sub(1))) as u32;
            point.dy = (f64::from(point.dy) * scale_y)
                .round()
                .min(f64::from(scaled.h.saturating_sub(1))) as u32;
        }
    }
    Ok((scaled, groups))
}

fn match_object(
    project: &ProjectFile,
    object: &ObjectRule,
    frame: &[u8],
    width: u32,
    height: u32,
) -> Result<MatchReport, String> {
    let (region, groups) = resolve_object(
        object,
        project.target.frame_width,
        project.target.frame_height,
        width,
        height,
    )?;
    validate_match_input(frame, width, height, &region, &groups)?;
    evaluate_groups_search(
        frame,
        width,
        height,
        &region,
        &groups,
        object.search_radius,
        object.scale_search_percent,
    )
}

fn print_json<T: Serialize>(value: &T) -> Result<(), String> {
    println!(
        "{}",
        serde_json::to_string_pretty(value).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn run(args: &[String]) -> Result<i32, String> {
    let command = args.first().map(String::as_str).unwrap_or("");
    match command {
        "validate" => {
            let path = PathBuf::from(args.get(1).ok_or("missing project path")?);
            let project = load_project(&path)?;
            for object in &project.objects {
                object.region.to_region()?;
            }
            print_json(&serde_json::json!({
                "valid": true,
                "version": project.version,
                "objects": project.objects.len(),
                "replayCases": project.replay_cases.len()
            }))?;
            Ok(0)
        }
        "match" => {
            let project_path = PathBuf::from(args.get(1).ok_or("missing project path")?);
            let selector = args.get(2).ok_or("missing object id or name")?;
            let frame_path = PathBuf::from(args.get(3).ok_or("missing PNG path")?);
            let project = load_project(&project_path)?;
            let object = project
                .objects
                .iter()
                .find(|item| item.id == *selector || item.name == *selector)
                .ok_or_else(|| format!("object not found: {selector}"))?;
            let (frame, width, height) = load_png_file(&frame_path)?;
            let report = match_object(&project, object, &frame, width, height)?;
            print_json(&MatchOutput {
                object_id: &object.id,
                object_name: &object.name,
                frame_width: width,
                frame_height: height,
                report,
            })?;
            Ok(0)
        }
        "test" => {
            let project_path = PathBuf::from(args.get(1).ok_or("missing project path")?);
            let project = load_project(&project_path)?;
            let mut results = Vec::new();
            let mut failures = 0usize;
            for sample in &project.replay_cases {
                let (frame, width, height) = load_replay_png(&project_path, sample)?;
                for expectation in &sample.expectations {
                    let object = project
                        .objects
                        .iter()
                        .find(|item| item.id == expectation.object_id)
                        .ok_or_else(|| {
                            format!(
                                "sample {} references missing object {}",
                                sample.id, expectation.object_id
                            )
                        })?;
                    let report = match_object(&project, object, &frame, width, height)?;
                    let actual = report
                        .groups
                        .iter()
                        .filter(|group| group.matched)
                        .map(|group| group.id.clone())
                        .collect::<Vec<_>>();
                    let passed = match &expectation.expected_group_id {
                        Some(expected) => actual.len() == 1 && actual.first() == Some(expected),
                        None => actual.is_empty(),
                    };
                    if !passed {
                        failures += 1;
                    }
                    results.push(serde_json::json!({
                        "sampleId": sample.id,
                        "sampleName": sample.name,
                        "objectId": object.id,
                        "expectedGroupId": expectation.expected_group_id,
                        "actualGroupIds": actual,
                        "passed": passed,
                        "elapsedMicros": report.elapsed_micros
                    }));
                }
            }
            print_json(&serde_json::json!({
                "passed": failures == 0,
                "assertions": results.len(),
                "failures": failures,
                "results": results
            }))?;
            Ok(if failures == 0 { 0 } else { 2 })
        }
        _ => Err(
            "usage: imagesitter-cli <validate PROJECT | match PROJECT OBJECT PNG | test PROJECT>"
                .into(),
        ),
    }
}

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    match run(&args) {
        Ok(code) => std::process::exit(code),
        Err(message) => {
            eprintln!("ImageSitter CLI error: {message}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_mode_resolves_region_and_point_coordinates() {
        let object: ObjectRule = serde_json::from_value(serde_json::json!({
            "id": "object",
            "name": "object",
            "region": { "left": 10, "top": 20, "right": 30, "bottom": 40 },
            "groups": [{
                "id": "state",
                "name": "state",
                "enabled": true,
                "points": [{
                    "dx": 5,
                    "dy": 10,
                    "reference": [1, 2, 3, 255],
                    "tolerance": [0, 0, 0],
                    "alphaMode": "ignore",
                    "alphaTolerance": 0,
                    "mustNot": false
                }],
                "minMatch": -1
            }],
            "coordinateMode": "scale",
            "anchorX": "start",
            "anchorY": "start",
            "searchRadius": 0,
            "scaleSearchPercent": 0
        }))
        .unwrap();
        let (region, groups) = resolve_object(&object, 100, 100, 200, 150).unwrap();
        assert_eq!((region.x, region.y, region.w, region.h), (20, 30, 40, 30));
        assert_eq!((groups[0].points[0].dx, groups[0].points[0].dy), (10, 15));
    }
}
