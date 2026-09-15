//! Read-only evidence for a proposed interaction. Evidence is sampled, never a
//! promise that a later event will reach or activate the element.
use crate::{Accessibility, accessibility::attribute, error};
use objc2_application_services::AXUIElement;
use objc2_core_foundation::CFBoolean;
use serde_json::{Value, json};
use unimation::{Effect, ElementRef, Point, Result, geometry::Rect};

fn native_bool(element: &AXUIElement, name: &str) -> Value {
    match attribute(element, name) {
        Ok(value) => match value.downcast_ref::<CFBoolean>() {
            Some(value) => json!({"state":"observed","value":value.as_bool(),"attribute":name}),
            None => json!({"state":"unknown","attribute":name,"error":
                error("native_type", "Expected CFBoolean", Effect::None)}),
        },
        Err(e) => json!({"state":"unknown","attribute":name,"error":e}),
    }
}

fn intersects(a: &Rect, b: &Rect) -> bool {
    a.valid()
        && b.valid()
        && a.x < b.x + b.width
        && b.x < a.x + a.width
        && a.y < b.y + b.height
        && b.y < a.y + a.height
}

fn check_result(result: Result<()>, negatives: &[&str]) -> Value {
    match result {
        Ok(()) => json!({"state":"passed"}),
        Err(e) => {
            json!({"state":if negatives.contains(&e.code.as_str()) {"failed"} else {"unknown"},"error":e})
        }
    }
}

impl Accessibility {
    /// Query native state and a current global center hit test without posting
    /// input, scrolling, activating, or changing focus. Unknown is not false.
    pub fn actionability(&mut self, target: &ElementRef) -> Result<Value> {
        let element = self.resolve(target)?;
        let enabled = native_bool(&element, "AXEnabled");
        let hidden = native_bool(&element, "AXHidden");
        let visible = native_bool(&element, "AXVisible");
        let minimized = native_bool(&element, "AXMinimized");
        let mut ancestors = Vec::new();
        let mut seen = vec![element.clone()];
        let mut current = element;
        let mut ancestry_end = json!({"state":"limited","limit":64});
        for _ in 0..64 {
            let parent = match attribute(&current, "AXParent") {
                Ok(value) => match value.downcast::<AXUIElement>() {
                    Ok(parent) => parent,
                    Err(_) => {
                        ancestry_end = json!({"state":"unknown","error":error("native_type", "AXParent is not an element", Effect::None)});
                        break;
                    }
                },
                Err(e) => {
                    ancestry_end = json!({"state":"stopped","error":e});
                    break;
                }
            };
            if seen.iter().any(|v| **v == *parent) {
                ancestry_end = json!({"state":"cycle"});
                break;
            }
            ancestors.push(json!({"reference":self.intern(&parent),
                "hidden":native_bool(&parent,"AXHidden"),
                "minimized":native_bool(&parent,"AXMinimized")}));
            seen.push(parent.clone());
            current = parent;
        }
        let sheet = check_result(self.require_sheet_access(target), &["blocked_by_modal"]);
        let geometry = self.element_bounds(target);
        let (bounds, display_intersection, center_hit) = match geometry {
            Ok(bounds) => {
                let intersection = match crate::capture::displays() {
                    Ok(displays) => {
                        let matches: Vec<_> = displays
                            .iter()
                            .filter(|d| intersects(&bounds, &d.bounds))
                            .map(|d| d.display_id)
                            .collect();
                        json!({"state":"observed","value":!matches.is_empty(),"display_ids":matches})
                    }
                    Err(e) => json!({"state":"unknown","error":e}),
                };
                let point = Point {
                    x: bounds.x + bounds.width / 2.,
                    y: bounds.y + bounds.height / 2.,
                };
                let result = check_result(
                    self.require_hit(target, &point),
                    &[
                        "occluded_or_moved",
                        "blocked_by_modal",
                        "outside_viewport",
                        "not_visible",
                        "disabled",
                    ],
                );
                (
                    json!({"state":"observed","value":bounds}),
                    intersection,
                    json!({"point":point,"result":result}),
                )
            }
            Err(e) => (
                json!({"state":"unknown","error":e}),
                json!({"state":"unknown","reason":"bounds unavailable"}),
                json!({"state":"unknown","reason":"bounds unavailable"}),
            ),
        };
        let actions = match self.inspect(target) {
            Ok(node) => json!({"state":"observed","actions":node["actions"]}),
            Err(e) => json!({"state":"unknown","error":e}),
        };
        Ok(json!({"reference":target,"effect":"none",
            "native":{"enabled":enabled,"hidden":hidden,"visible":visible,"minimized":minimized},
            "ancestors":ancestors,"ancestry_end":ancestry_end,
            "bounds":bounds,"display_intersection":display_intersection,
            "ancestor_clipping":{"state":"unknown","reason":"AX bounds do not establish ancestor clipping"},
            "sheet_check_scope":"Checks advertised attached sheets only; unavailable sheet attributes may prevent detection of a blocker",
            "routes":{
                "global":{"center_hit_test":center_hit,"sheet_check":sheet,
                    "scope":"sampled center only; not full occlusion or an atomic dispatch guarantee"},
                "skylight":{"state":"not_tested","sheet_check":sheet,
                    "scope":"global occlusion does not determine process-targeted delivery; window ownership and route support are checked at dispatch"},
                "semantic":{"advertised":actions,"scope":"advertised actions do not prove successful activation or require global pointer visibility"}
            },
            "note":"No aggregate clickable boolean. Native visibility, geometry, enablement and route-specific evidence are independent and may change after this query."
        }))
    }
}

impl unimation::Actionability for Accessibility {
    type Report = Value;
    fn actionability(&mut self, target: &ElementRef) -> Result<Self::Report> {
        Accessibility::actionability(self, target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn display_intersection_handles_negative_origins_and_edge_contact() {
        let display = Rect {
            x: -100.,
            y: 0.,
            width: 100.,
            height: 100.,
        };
        assert!(intersects(
            &display,
            &Rect {
                x: -2.,
                y: 20.,
                width: 4.,
                height: 4.
            }
        ));
        assert!(!intersects(
            &display,
            &Rect {
                x: 0.,
                y: 20.,
                width: 4.,
                height: 4.
            }
        ));
        assert!(!intersects(
            &display,
            &Rect {
                x: -2.,
                y: 20.,
                width: 0.,
                height: 4.
            }
        ));
    }
    #[test]
    fn native_failures_are_not_negative_evidence() {
        assert_eq!(
            check_result(
                Err(error("ax_-25202", "timeout", Effect::None)),
                &["occluded_or_moved"]
            )["state"],
            "unknown"
        );
        assert_eq!(
            check_result(
                Err(error("occluded_or_moved", "blocked", Effect::None)),
                &["occluded_or_moved"]
            )["state"],
            "failed"
        );
    }
}
