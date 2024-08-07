//! V4L2R bindings generator

include!("../lib/bindgen.rs");

fn main() {
    bindgen_cmd::build(v4l2r_bindgen_builder)
}
