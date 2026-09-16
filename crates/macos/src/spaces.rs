//! Read-only Space discovery. IDs are session-local WindowServer identities,
//! not Mission Control ordinals. Unsupported private APIs report unknown state.
use crate::{capture, error};
use actuate::{Effect, NativeError, Result};
use objc2_core_foundation::{
    CFArray, CFBoolean, CFDictionary, CFNumber, CFRetained, CFString, CFType, CFUUID,
};
use objc2_core_graphics::{CGWindowListCopyWindowInfo, CGWindowListOption};
use serde::{Deserialize, Serialize};
use std::{ffi::c_void, ptr::NonNull};

type DisplayUuid = unsafe extern "C" fn(u32) -> *mut CFUUID;
type Connection = unsafe extern "C" fn() -> i32;
type CopyDisplays = unsafe extern "C" fn(i32) -> *mut CFArray;
type CopySpaces = unsafe extern "C" fn(i32, i32, *const CFArray<CFNumber>) -> *mut CFArray;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Space {
    pub id: u64,
    pub native_type: i64,
    pub uuid: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplaySpaces {
    pub display_id: Option<u32>,
    pub display_uuid: String,
    pub current_space_id: u64,
    pub spaces: Vec<Space>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpaceTopology {
    pub displays: Vec<DisplaySpaces>,
    pub private_api: bool,
}
impl SpaceTopology {
    /// Empty or unknown membership must not be interpreted as an inactive Space.
    pub fn active_membership(&self, ids: &[u64]) -> Option<bool> {
        if ids.is_empty() || self.displays.is_empty() {
            return None;
        }
        if ids
            .iter()
            .any(|id| self.displays.iter().any(|d| d.current_space_id == *id))
        {
            return Some(true);
        }
        ids.iter()
            .all(|id| {
                self.displays
                    .iter()
                    .any(|d| d.spaces.iter().any(|s| s.id == *id))
            })
            .then_some(false)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowSpaces {
    pub ids: Vec<u64>,
    pub on_active_space: Option<bool>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowDiscovery {
    #[serde(flatten)]
    pub geometry: capture::WindowGeometry,
    pub on_screen: Option<bool>,
    /// Intersecting display rectangles, not proof of a window's Space assignment.
    pub display_ids: Vec<u32>,
    pub spaces: Option<WindowSpaces>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space_error: Option<NativeError>,
}

pub struct SpaceQuery {
    handle: *mut c_void,
    connection: Connection,
    copy_displays: CopyDisplays,
    copy_spaces: CopySpaces,
    display_uuid: Option<DisplayUuid>,
}
fn failure(message: &str) -> NativeError {
    error("space_query_unavailable", message, Effect::None)
}
impl SpaceQuery {
    pub fn new() -> Result<Self> {
        // SAFETY: exact signatures used by yabai; handle owns function lifetimes.
        unsafe {
            let handle = libc::dlopen(
                c"/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight".as_ptr(),
                libc::RTLD_NOW | libc::RTLD_LOCAL,
            );
            if handle.is_null() {
                return Err(failure("SkyLight unavailable"));
            }
            let connection = libc::dlsym(handle, c"SLSMainConnectionID".as_ptr());
            let displays = libc::dlsym(handle, c"SLSCopyManagedDisplaySpaces".as_ptr());
            let spaces = libc::dlsym(handle, c"SLSCopySpacesForWindows".as_ptr());
            if [connection, displays, spaces].iter().any(|p| p.is_null()) {
                libc::dlclose(handle);
                return Err(failure("Space discovery symbols unavailable"));
            }
            let display_uuid = libc::dlsym(
                libc::RTLD_DEFAULT,
                c"CGDisplayCreateUUIDFromDisplayID".as_ptr(),
            );
            Ok(Self {
                display_uuid: (!display_uuid.is_null())
                    .then(|| std::mem::transmute::<*mut c_void, DisplayUuid>(display_uuid)),
                handle,
                connection: std::mem::transmute::<*mut c_void, Connection>(connection),
                copy_displays: std::mem::transmute::<*mut c_void, CopyDisplays>(displays),
                copy_spaces: std::mem::transmute::<*mut c_void, CopySpaces>(spaces),
            })
        }
    }
    pub fn topology(&self) -> Result<SpaceTopology> {
        // SAFETY: Copy follows CF ownership; runtime types are checked below.
        let raw = unsafe { (self.copy_displays)((self.connection)()) };
        let array = unsafe {
            CFRetained::from_raw(
                NonNull::new(raw).ok_or_else(|| failure("Space topology unavailable"))?,
            )
        };
        let display_ids = capture::displays()?
            .into_iter()
            .filter_map(|d| {
                let copy = self.display_uuid?;
                // SAFETY: Copy returns an owned UUID or null for an unavailable display.
                let uuid = unsafe { CFRetained::from_raw(NonNull::new(copy(d.display_id))?) };
                Some((
                    CFUUID::new_string(None, Some(&uuid))?.to_string(),
                    d.display_id,
                ))
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        let mut displays = Vec::new();
        for index in 0..array.count() {
            let dict = object(&array, index)
                .downcast_ref::<CFDictionary>()
                .ok_or_else(|| failure("Invalid display record"))?;
            let uuid = value(dict, "Display Identifier")
                .and_then(|v| v.downcast_ref::<CFString>())
                .ok_or_else(|| failure("Missing display identifier"))?
                .to_string();
            let current = value(dict, "Current Space")
                .and_then(|v| v.downcast_ref::<CFDictionary>())
                .and_then(|d| number(d, "ManagedSpaceID"))
                .and_then(|id| u64::try_from(id).ok())
                .ok_or_else(|| failure("Missing current Space"))?;
            let rows = value(dict, "Spaces")
                .and_then(|v| v.downcast_ref::<CFArray>())
                .ok_or_else(|| failure("Missing Space list"))?;
            let mut spaces = Vec::new();
            for index in 0..rows.count() {
                let row = object(rows, index)
                    .downcast_ref::<CFDictionary>()
                    .ok_or_else(|| failure("Invalid Space record"))?;
                let id = number(row, "ManagedSpaceID")
                    .and_then(|id| u64::try_from(id).ok())
                    .ok_or_else(|| failure("Invalid Space ID"))?;
                spaces.push(Space {
                    id,
                    native_type: number(row, "type")
                        .ok_or_else(|| failure("Missing Space type"))?,
                    uuid: value(row, "uuid")
                        .and_then(|v| v.downcast_ref::<CFString>())
                        .map(|s| s.to_string()),
                });
            }
            displays.push(DisplaySpaces {
                display_id: display_ids.get(&uuid).copied(),
                display_uuid: uuid,
                current_space_id: current,
                spaces,
            });
        }
        Ok(SpaceTopology {
            displays,
            private_api: true,
        })
    }
    pub fn window(&self, window_id: u32, topology: &SpaceTopology) -> Result<WindowSpaces> {
        let windows = CFArray::from_retained_objects(&[CFNumber::new_i64(i64::from(window_id))]);
        // Selector 7 asks for all memberships, including inactive Spaces.
        let raw = unsafe { (self.copy_spaces)((self.connection)(), 7, &*windows) };
        let array = unsafe {
            CFRetained::from_raw(
                NonNull::new(raw).ok_or_else(|| failure("Window Space membership unavailable"))?,
            )
        };
        let mut ids = Vec::new();
        for index in 0..array.count() {
            let id = object(&array, index)
                .downcast_ref::<CFNumber>()
                .and_then(|n| n.as_i64())
                .and_then(|n| u64::try_from(n).ok())
                .ok_or_else(|| failure("Invalid Space membership"))?;
            ids.push(id);
        }
        ids.sort_unstable();
        ids.dedup();
        Ok(WindowSpaces {
            on_active_space: topology.active_membership(&ids),
            ids,
        })
    }
}
impl Drop for SpaceQuery {
    fn drop(&mut self) {
        unsafe {
            libc::dlclose(self.handle);
        }
    }
}
fn object(array: &CFArray, index: isize) -> &CFType {
    unsafe { &*array.value_at_index(index).cast::<CFType>() }
}
fn value<'a>(dict: &'a CFDictionary, name: &str) -> Option<&'a CFType> {
    let key = CFString::from_str(name);
    let raw = unsafe { dict.value((&*key as *const CFString).cast()) };
    NonNull::new(raw.cast_mut()).map(|p| unsafe { &*p.as_ptr().cast::<CFType>() })
}
fn number(dict: &CFDictionary, key: &str) -> Option<i64> {
    value(dict, key)?.downcast_ref::<CFNumber>()?.as_i64()
}
pub fn windows() -> Result<Vec<WindowDiscovery>> {
    let geometry = capture::windows()?;
    let displays = capture::displays()?;
    let query = SpaceQuery::new();
    let topology = query
        .as_ref()
        .map_err(Clone::clone)
        .and_then(|q| q.topology());
    let mut result = Vec::new();
    for window in geometry {
        let on_screen =
            CGWindowListCopyWindowInfo(CGWindowListOption::OptionIncludingWindow, window.window_id)
                .and_then(|a| {
                    if a.count() != 1 {
                        return None;
                    }
                    let dict = object(&a, 0).downcast_ref::<CFDictionary>()?;
                    value(dict, "kCGWindowIsOnscreen")?
                        .downcast_ref::<CFBoolean>()
                        .map(|b| b.value())
                });
        let spaces = query.as_ref().map_err(Clone::clone).and_then(|q| {
            topology
                .as_ref()
                .map_err(Clone::clone)
                .and_then(|t| q.window(window.window_id, t))
        });
        let b = &window.bounds;
        let display_ids = displays
            .iter()
            .filter(|d| {
                b.x < d.bounds.x + d.bounds.width
                    && b.x + b.width > d.bounds.x
                    && b.y < d.bounds.y + d.bounds.height
                    && b.y + b.height > d.bounds.y
            })
            .map(|d| d.display_id)
            .collect();
        let (spaces, space_error) = match spaces {
            Ok(s) => (Some(s), None),
            Err(e) => (None, Some(e)),
        };
        result.push(WindowDiscovery {
            geometry: window,
            on_screen,
            display_ids,
            spaces,
            space_error,
        });
    }
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn membership_uses_all_displays_and_preserves_unknown() {
        let topology = SpaceTopology {
            private_api: true,
            displays: vec![
                DisplaySpaces {
                    display_id: Some(1),
                    display_uuid: "a".into(),
                    current_space_id: 1,
                    spaces: vec![
                        Space {
                            id: 1,
                            native_type: 0,
                            uuid: None,
                        },
                        Space {
                            id: 2,
                            native_type: 0,
                            uuid: None,
                        },
                    ],
                },
                DisplaySpaces {
                    display_id: Some(2),
                    display_uuid: "b".into(),
                    current_space_id: 3,
                    spaces: vec![Space {
                        id: 3,
                        native_type: 0,
                        uuid: None,
                    }],
                },
            ],
        };
        assert_eq!(topology.active_membership(&[3]), Some(true));
        assert_eq!(topology.active_membership(&[2]), Some(false));
        assert_eq!(topology.active_membership(&[1, 2, 3]), Some(true));
        assert_eq!(topology.active_membership(&[]), None);
        assert_eq!(topology.active_membership(&[99]), None);
    }
}
