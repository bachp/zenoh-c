//
// Copyright (c) 2017, 2022 ZettaScale Technology.
//
// This program and the accompanying materials are made available under the
// terms of the Eclipse Public License 2.0 which is available at
// http://www.eclipse.org/legal/epl-2.0, or the Apache License, Version 2.0
// which is available at https://www.apache.org/licenses/LICENSE-2.0.
//
// SPDX-License-Identifier: EPL-2.0 OR Apache-2.0
//
// Contributors:
//   ZettaScale Zenoh team, <zenoh@zettascale.tech>
//

use async_std::task;
use libc::{c_char, c_uint, c_ulong, size_t};
use std::ffi::CString;
use zenoh::scouting::Hello;
use zenoh_protocol_core::{whatami::WhatAmIMatcher, WhatAmI};
use zenoh_util::core::AsyncResolve;

use crate::{z_closure_hello_call, z_id_t, z_owned_closure_hello_t, z_owned_config_t, Z_ROUTER};

/// An owned array of owned, zenoh allocated, NULL terminated strings.
///
/// Note that `val`
///
/// Like all `z_owned_X_t`, an instance will be destroyed by any function which takes a mutable pointer to said instance, as this implies the instance's inners were moved.
/// To make this fact more obvious when reading your code, consider using `z_move(val)` instead of `&val` as the argument.
/// After a move, `val` will still exist, but will no longer be valid. The destructors are double-drop-safe, but other functions will still trust that your `val` is valid.
///
/// To check if `val` is still valid, you may use `z_X_check(&val)` or `z_check(val)` if your compiler supports `_Generic`, which will return `true` if `val` is valid.
#[repr(C)]
pub struct z_owned_str_array_t {
    pub val: *mut *mut c_char,
    pub len: size_t,
}

/// Frees `strs` and invalidates it for double-drop safety.
#[allow(clippy::missing_safety_doc)]
#[no_mangle]
pub unsafe extern "C" fn z_str_array_drop(strs: &mut z_owned_str_array_t) {
    let locators = Vec::from_raw_parts(
        strs.val as *mut *const c_char,
        strs.len as usize,
        strs.len as usize,
    );
    for locator in locators {
        std::mem::drop(CString::from_raw(locator as *mut c_char));
    }
    strs.val = std::ptr::null_mut();
    strs.len = 0;
}

/// Returns ``true`` if `strs` is valid.
#[allow(clippy::missing_safety_doc)]
#[no_mangle]
pub unsafe extern "C" fn z_str_array_check(strs: &z_owned_str_array_t) -> bool {
    !strs.val.is_null()
}

/// A zenoh-allocated hello message returned by a zenoh entity to a scout message sent with `z_scout`.
///
/// Members:
///   unsigned int whatami: The kind of zenoh entity.
///   z_owned_bytes_t pid: The peer id of the scouted entity (empty if absent).
///   z_owned_str_array_t locators: The locators of the scouted entity.
///
/// Like all `z_owned_X_t`, an instance will be destroyed by any function which takes a mutable pointer to said instance, as this implies the instance's inners were moved.
/// To make this fact more obvious when reading your code, consider using `z_move(val)` instead of `&val` as the argument.
/// After a move, `val` will still exist, but will no longer be valid. The destructors are double-drop-safe, but other functions will still trust that your `val` is valid.
///
/// To check if `val` is still valid, you may use `z_X_check(&val)` (or `z_check(val)` if your compiler supports `_Generic`), which will return `true` if `val` is valid.
#[repr(C)]
pub struct z_owned_hello_t {
    pub whatami: c_uint,
    pub pid: z_id_t,
    pub locators: z_owned_str_array_t,
}
impl From<Hello> for z_owned_hello_t {
    fn from(h: Hello) -> Self {
        z_owned_hello_t {
            whatami: match h.whatami {
                Some(whatami) => whatami as c_uint,
                None => Z_ROUTER,
            },
            pid: match h.zid {
                Some(id) => unsafe { std::mem::transmute(id) },
                None => z_id_t { id: [0; 16] },
            },
            locators: match h.locators {
                Some(locators) => {
                    let mut locators = locators
                        .into_iter()
                        .map(|l| CString::new(l.to_string()).unwrap().into_raw())
                        .collect::<Vec<_>>();
                    let val = locators.as_mut_ptr();
                    let len = locators.len();
                    std::mem::forget(locators);
                    z_owned_str_array_t { val, len }
                }
                None => z_owned_str_array_t {
                    val: std::ptr::null_mut(),
                    len: 0,
                },
            },
        }
    }
}

/// Frees `hello`, invalidating it for double-drop safety.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn z_hello_drop(hello: &mut z_owned_hello_t) {
    z_str_array_drop(&mut hello.locators);
    hello.whatami = 0;
}

/// Constructs a gravestone value for hello, useful to steal one from a callback
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn z_hello_null() -> z_owned_hello_t {
    z_owned_hello_t {
        whatami: 0,
        pid: z_id_t { id: [0; 16] },
        locators: z_owned_str_array_t {
            val: std::ptr::null_mut(),
            len: 0,
        },
    }
}
impl Drop for z_owned_hello_t {
    fn drop(&mut self) {
        unsafe { z_hello_drop(self) };
    }
}
/// Returns ``true`` if `hello` is valid.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn z_hello_check(hello: &z_owned_hello_t) -> bool {
    hello.whatami != 0 && z_str_array_check(&hello.locators)
}

/// Scout for routers and/or peers.
///
/// Parameters:
///     what: A whatami bitmask of zenoh entities kind to scout for.
///     config: A set of properties to configure the scouting.
///     timeout: The time (in milliseconds) that should be spent scouting.
///
/// Returns:
///     An array of `z_hello_t` messages.
#[allow(clippy::missing_safety_doc)]
#[no_mangle]
pub unsafe extern "C" fn z_scout(
    what: c_uint,
    config: &mut z_owned_config_t,
    callback: &mut z_owned_closure_hello_t,
    timeout: c_ulong,
) {
    let what = WhatAmIMatcher::try_from(what as u64).unwrap_or(WhatAmI::Router | WhatAmI::Peer);
    let config = config.as_mut().take().expect("invalid config");
    let mut closure = z_owned_closure_hello_t::empty();
    std::mem::swap(&mut closure, callback);

    task::block_on(async move {
        let scout = zenoh::scout(what, *config)
            .callback(move |h| {
                let mut hello = h.into();
                z_closure_hello_call(&closure, &mut hello)
            })
            .res_async()
            .await
            .unwrap();
        async_std::task::sleep(std::time::Duration::from_millis(timeout as u64)).await;
        std::mem::drop(scout);
    });
}
