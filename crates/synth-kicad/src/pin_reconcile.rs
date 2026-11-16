// SPDX-License-Identifier: Apache-2.0

//! Physical-pin reconciliation lives in `synth-layout`, which sizes group
//! boxes from the power legs and no-connect markers it finds. This module
//! re-exports it and adds the one piece only the exporter needs, the
//! s-expression for a no-connect marker.

use crate::sexp::{Sexp, num};

pub use synth_layout::pin_reconcile::*;

/// Render a `(no_connect (at x y) (uuid ...))` s-expression for a pin
/// terminal. `project` namespaces a deterministic uuid from `key`.
#[must_use]
pub fn no_connect_sexp(x: f64, y: f64, uuid: &str) -> Sexp {
    Sexp::list(
        "no_connect",
        vec![
            Sexp::list("at", vec![num(x), num(y)]),
            Sexp::list("uuid", vec![Sexp::str(uuid)]),
        ],
    )
}
