use clap::Parser;
use std::fs;

#[derive(Parser)]
struct Cli {
    /// Sets a custom input file
    #[arg(short, long, value_name = "INPUT FILE", default_value = "fix753.h")]
    input_file: String,

    /// Sets a custom output file
    #[arg(short, long, value_name = "OUTPUT FILE", default_value = "bindings.rs")]
    output_file: String,

    /// Sets the include file for which to generate bindings
    #[arg(
        short,
        long,
        value_name = "BINDINGS FOR",
        default_value = "linux/videodev2.h"
    )]
    bindings_for: String,

    /// Flag to keep the actual input presented to bindgen
    #[arg(short, long, default_value_t = false)]
    keep_actual_input: bool,

    /// Sets include path for clang
    #[arg(
        short = 'I',
        long,
        value_name = "INCLUDE PATH",
        default_value = "/usr/include"
    )]
    include_path: String,

    /// Sets sysroot for clang
    #[arg(short, long, value_name = "SYSROOT PATH", default_value = "/")]
    sysroot: String,

    /// Sets target for clang
    #[arg(short = 't', long, value_name = "CLANG TARGET")]
    clang_target: Option<String>,
}

#[derive(Debug)]
pub struct Fix753;
impl bindgen::callbacks::ParseCallbacks for Fix753 {
    fn item_name(&self, original_item_name: &str) -> Option<String> {
        Some(original_item_name.trim_start_matches("Fix753_").to_owned())
    }
}

const ACTUAL_BINDGEN_INPUT_FILE: &str = "actual_bindgen_input.h";

fn main() {
    let cli = Cli::parse();

    let mut include_path = "-I".to_owned();
    include_path.push_str(&cli.include_path);

    let mut sysroot = "--sysroot=".to_owned();
    sysroot.push_str(&cli.sysroot);

    let mut clang_args = vec![include_path, sysroot];
    if let Some(clang_target) = cli.clang_target {
        let mut target = "--target=".to_owned();
        target.push_str(&clang_target);
        clang_args.push(target);
    }

    println!("input file:{}", cli.input_file);
    println!("output file:{}", cli.output_file);
    println!("bindings for:{}", cli.bindings_for);
    println!("clang args:{:?}", clang_args);

    let fix_file = fs::read_to_string(cli.input_file).expect("Unable to read file");

    let mut actual_input = "#include \"".to_owned();
    actual_input.push_str(&cli.include_path);
    actual_input.push('/');
    actual_input.push_str(&cli.bindings_for);
    actual_input.push('"');
    actual_input.push('\n');
    actual_input
        .push_str("#define MARK_FIX_753(name) const unsigned long int Fix753_##name = name;");
    actual_input.push('\n');
    actual_input.push_str(&fix_file);

    fs::write(ACTUAL_BINDGEN_INPUT_FILE, actual_input).expect("Unable to write to file");

    let bindings = bindgen::Builder::default()
        .header(ACTUAL_BINDGEN_INPUT_FILE)
        .parse_callbacks(Box::new(Fix753 {}))
        .derive_partialeq(true)
        .derive_eq(true)
        .derive_default(true)
        .clang_args(clang_args)
        .generate()
        .expect("Unable to generate bindings");

    bindings
        .write_to_file(cli.output_file)
        .expect("Couldn't write bindings");

    if !cli.keep_actual_input {
        fs::remove_file(ACTUAL_BINDGEN_INPUT_FILE).expect("Unable to remove actual bindgen input");
    }
}
