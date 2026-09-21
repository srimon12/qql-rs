//! Geo / slice lowering: plan geo shapes and slice params onto edge types.
//!
//! Pure move from `filter_converter.rs` (size hygiene split).

use std::num::NonZeroU32;

use qdrant_edge::external::ordered_float::OrderedFloat;
use qdrant_edge::{
    GeoBoundingBox, GeoLineString, GeoPoint, GeoPolygon, GeoRadius, Slice, SliceCondition,
};
use qql_core::error::QqlError;
use qql_plan::types::{
    GeoBoundingBox as PlanGeoBoundingBox, GeoLineString as PlanGeoLineString,
    GeoPoint as PlanGeoPoint, GeoPolygon as PlanGeoPolygon, GeoRadius as PlanGeoRadius,
    SliceCondition as PlanSliceCondition,
};

use super::filter_error;

pub(crate) fn lower_geo_bounding_box(
    polygon: &PlanGeoBoundingBox,
) -> Result<GeoBoundingBox, QqlError> {
    Ok(GeoBoundingBox {
        top_left: lower_geo_point(&polygon.top_left),
        bottom_right: lower_geo_point(&polygon.bottom_right),
    })
}

pub(crate) fn lower_geo_radius(radius: &PlanGeoRadius) -> Result<GeoRadius, QqlError> {
    Ok(GeoRadius {
        center: lower_geo_point(&radius.center),
        radius: OrderedFloat(radius.radius),
    })
}

/// Build a validated edge polygon. The wire path validated through
/// `TryFrom<GeoPolygonShadow>`; call the same validator here so direct struct
/// construction cannot smuggle in an unclosed or undersized ring.
pub(crate) fn lower_geo_polygon(polygon: &PlanGeoPolygon) -> Result<GeoPolygon, QqlError> {
    let exterior = lower_geo_line(&polygon.exterior);
    GeoPolygon::validate_line_string(&exterior)
        .map_err(|error| filter_error(format!("invalid geo_polygon exterior: {error}")))?;
    let interiors: Vec<GeoLineString> = polygon.interiors.iter().map(lower_geo_line).collect();
    for interior in &interiors {
        GeoPolygon::validate_line_string(interior)
            .map_err(|error| filter_error(format!("invalid geo_polygon interior: {error}")))?;
    }
    Ok(GeoPolygon {
        exterior,
        interiors: Some(interiors),
    })
}

fn lower_geo_line(line: &PlanGeoLineString) -> GeoLineString {
    GeoLineString {
        points: line.points.iter().map(lower_geo_point).collect(),
    }
}

fn lower_geo_point(point: &PlanGeoPoint) -> GeoPoint {
    GeoPoint {
        lat: OrderedFloat(point.lat),
        lon: OrderedFloat(point.lon),
    }
}

pub(crate) fn lower_slice(condition: &PlanSliceCondition) -> Result<SliceCondition, QqlError> {
    let total = u32::try_from(condition.slice.total)
        .ok()
        .and_then(NonZeroU32::new)
        .ok_or_else(|| {
            filter_error(format!(
                "slice total must be in 1..=u32::MAX, got {}",
                condition.slice.total
            ))
        })?;
    let index = u32::try_from(condition.slice.index).map_err(|_| {
        filter_error(format!(
            "slice index {} exceeds u32::MAX",
            condition.slice.index
        ))
    })?;
    Ok(SliceCondition {
        slice: Slice { total, index },
    })
}
