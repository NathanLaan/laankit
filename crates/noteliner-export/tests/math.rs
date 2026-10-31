//! Math through Typst: every supported LaTeX command and construct converts
//! to Typst that compiles, generated formulas always compile, unsupported
//! input is refused rather than mis-rendered, and PDFs typeset math natively.

#![cfg(feature = "pdf")]

use std::sync::OnceLock;

use noteliner_export::math::latex_to_typst;
use noteliner_export::{ExportOptions, Exporter, Title};

fn exporter() -> &'static Exporter {
    static EXPORTER: OnceLock<Exporter> = OnceLock::new();
    EXPORTER.get_or_init(|| {
        Exporter::new(
            laankit_egui::fonts::bundled_faces()
                .iter()
                .map(|(_, bytes)| *bytes),
        )
        .with_system_fallback(false)
    })
}

fn svg(latex: &str, display: bool) -> Result<String, String> {
    exporter()
        .math_svg(latex, display, 16.0)
        .map_err(|e| e.to_string())
}

/// The LaTeX commands in `math.rs`'s symbol table, read from the source so a
/// new entry is tested without listing it twice.
fn symbol_commands() -> Vec<String> {
    let src = include_str!("../src/math.rs");
    let start = src.find("fn symbol(name").expect("symbol table");
    let end = start + src[start..].find("_ => return None").expect("table end");
    let quoted = regex_lite_quoted(&src[start..end]);
    quoted.into_iter().filter(|s| !s.is_empty()).collect()
}

/// The left-hand `"name"` patterns of each `"a" | "b" => "typst",` arm.
fn regex_lite_quoted(table: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in table.lines() {
        let Some((lhs, _)) = line.split_once("=>") else {
            continue;
        };
        for part in lhs.split('|') {
            let part = part.trim();
            if let Some(name) = part.strip_prefix('"').and_then(|p| p.strip_suffix('"')) {
                out.push(name.replace("\\\\", "\\"));
            }
        }
    }
    out
}

#[test]
fn every_symbol_compiles() {
    let commands = symbol_commands();
    assert!(commands.len() > 150, "found {} symbols", commands.len());
    let mut failures = Vec::new();
    for name in &commands {
        // Each symbol between two operands, so spacing commands and operators
        // have something to sit between.
        let latex = format!("a \\{name} b");
        if let Err(e) = svg(&latex, false) {
            failures.push(format!("\\{name}: {e}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const CONSTRUCTS: &[&str] = &[
    "x^2 + y^2 = z^2",
    "x_{i+1}^{2n}",
    "f'(x) = \\lim_{h \\to 0} \\frac{f(x+h) - f(x)}{h}",
    "\\frac{a}{b} + \\dfrac{1}{2} + \\tfrac34 + \\binom{n}{k}",
    "\\sqrt{x} + \\sqrt[3]{x^2}",
    "\\sum_{i=1}^{n} i = \\frac{n(n+1)}{2}",
    "\\int_0^\\infty e^{-x^2}\\,dx = \\frac{\\sqrt{\\pi}}{2}",
    "\\prod_{p \\text{ prime}} \\frac{1}{1 - p^{-s}}",
    "\\oint_C \\vec{F} \\cdot d\\vec{r}",
    "\\left( \\frac{a}{b} \\right)^2 \\left[ x \\right] \\left\\{ y \\right\\} \\left| z \\right|",
    "\\left\\langle u, v \\right\\rangle \\left\\lfloor x \\right\\rfloor \\left. \\frac{df}{dx} \\right|_{x=0}",
    "\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}",
    "\\begin{bmatrix} 1 & 0 \\\\ 0 & 1 \\end{bmatrix} \\begin{vmatrix} a & b \\\\ c & d \\end{vmatrix}",
    "\\begin{matrix} 1 & 2 \\\\ 3 & 4 \\end{matrix} \\begin{Bmatrix} x \\end{Bmatrix} \\begin{Vmatrix} y \\end{Vmatrix}",
    "|x| = \\begin{cases} x & x \\ge 0 \\\\ -x & \\text{otherwise} \\end{cases}",
    "\\begin{aligned} (a+b)^2 &= a^2 + 2ab + b^2 \\\\ &\\ge 0 \\end{aligned}",
    "\\begin{gathered} a = b \\\\ c = d \\end{gathered}",
    "\\text{area} = \\pi r^2, \\quad \\mathbb{R}, \\mathcal{L}, \\mathfrak{g}, \\mathbf{v}, \\mathrm{d}x, \\mathsf{A}, \\mathtt{x}, \\boldsymbol{\\mu}",
    "\\hat{x}\\bar{y}\\tilde{z}\\vec{v}\\dot{a}\\ddot{b}\\overline{AB}\\underline{c}\\widehat{xy}\\widetilde{ab}\\acute{e}\\grave{a}\\breve{u}\\check{c}",
    "\\overbrace{1+2+3}^{6} \\underbrace{a+b}_{c} \\overbrace{x}",
    "\\overset{!}{=} \\underset{n}{\\max} \\stackrel{def}{=}",
    "\\operatorname{rank}(A) \\operatorname*{argmax}_x f(x)",
    "a \\not= b, x \\not\\in S, A \\not\\subset B",
    "a \\equiv b \\pmod{n}, \\; a \\bmod b",
    "\\big( \\Big[ \\bigg\\{ x \\Bigg\\} \\Big] \\big) \\displaystyle \\sum \\textstyle x",
    "1,000; 3.14 \\cdot 10^{-3}",
    "a / b, a * b, a' b'' \"q\" @ ~ c",
    "αβγ → ∞",
    "\\text{a \"quoted\" \\{word\\}}",
];

#[test]
fn constructs_compile() {
    let mut failures = Vec::new();
    for latex in CONSTRUCTS {
        for display in [false, true] {
            match svg(latex, display) {
                Ok(s) => assert!(s.contains("<svg"), "{latex}: not SVG"),
                Err(e) => failures.push(format!(
                    "{latex} (display {display}): {e}\n  typst: {:?}",
                    latex_to_typst(latex)
                )),
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn svg_is_white_on_transparent_and_sized_by_the_text() {
    let small = exporter().math_svg("x^2", false, 10.0).unwrap();
    let large = exporter().math_svg("x^2", false, 40.0).unwrap();
    assert!(
        small.contains("#ffffff") || small.contains("white"),
        "{small}"
    );
    let width = |s: &str| -> f32 {
        let i = s.find("width=\"").expect("width") + 7;
        s[i..]
            .split(|c: char| !c.is_ascii_digit() && c != '.')
            .next()
            .unwrap()
            .parse()
            .unwrap()
    };
    assert!(width(&large) > width(&small) * 3.0);
}

#[test]
fn unsupported_is_refused_not_misrendered() {
    for latex in [
        "\\color{red}{x}",
        "\\boxed{x}",
        "\\begin{array}{c} a \\end{array}",
        "\\frac{a}",
    ] {
        assert!(svg(latex, false).is_err(), "{latex}");
    }
}

/// A tiny grammar of supported LaTeX; every sentence must compile.
#[test]
fn generated_formulas_always_compile() {
    let mut seed = 0x9E3779B97F4A7C15u64;
    let mut rand = move |n: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % n as u64) as usize
    };
    fn expr(rand: &mut dyn FnMut(usize) -> usize, depth: usize) -> String {
        let leaf = [
            "x",
            "y",
            "2",
            "\\alpha",
            "\\pi",
            "\\infty",
            "n",
            "3.5",
            "\\partial f",
        ];
        if depth == 0 {
            return leaf[rand(leaf.len())].to_string();
        }
        let a = expr(rand, depth - 1);
        let b = expr(rand, depth - 1);
        match rand(14) {
            0 => format!("{a} + {b}"),
            1 => format!("{a} - {b}"),
            2 => format!("{a}^{{{b}}}"),
            3 => format!("{a}_{{{b}}}"),
            4 => format!("\\frac{{{a}}}{{{b}}}"),
            5 => format!("\\sqrt{{{a}}}"),
            6 => format!("\\left( {a} \\right)"),
            7 => format!("\\sum_{{{a}}}^{{{b}}} {a}"),
            8 => format!("\\int_{{{a}}}^{{{b}}} {b} \\, dx"),
            9 => format!("\\begin{{pmatrix}} {a} & {b} \\\\ {b} & {a} \\end{{pmatrix}}"),
            10 => format!("\\text{{if }} {a} \\le {b}"),
            11 => format!("\\hat{{{a}}} \\cdot \\mathbf{{{b}}}"),
            12 => format!("{a}, {b}; {a}"),
            _ => format!("\\begin{{cases}} {a} & {b} \\\\ {b} & \\text{{else}} \\end{{cases}}"),
        }
    }
    let mut failures = Vec::new();
    for _ in 0..200 {
        let depth = 1 + rand(3);
        let latex = expr(&mut rand, depth);
        if let Err(e) = svg(&latex, rand(2) == 0) {
            failures.push(format!("{latex}: {e}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn pdf_typesets_math_natively() {
    let note = "# Maths\n\nEuler: $e^{i\\pi} + 1 = 0$.\n\n$$\\int_0^1 x^2\\,dx = \\frac{1}{3}$$\n\n```math\n\\sum_{i=1}^n i\n```\n\nUnsupported: $\\color{red}{x}$ stays.\n";
    let options = ExportOptions {
        title: Title::Text("Maths".into()),
        ..Default::default()
    };
    let source = noteliner_export::typst_source(note, &options);
    assert!(source.contains("$e^(i pi) plus 1 eq 0$"), "{source}");
    assert!(source.contains("$ integral_(0)^(1)"), "{source}");
    assert!(source.contains("$ sum_(i eq 1)^(n) i $"), "{source}");
    assert!(source.contains("#raw(\"$\\\\color{red}{x}$\")"), "{source}");
    let pdf = exporter().export(note, &options).expect("export");
    assert!(pdf.starts_with(b"%PDF-"));
    let has_math_font = pdf.windows(9).any(|w| w == b"NewCMMath");
    assert!(has_math_font, "the math font isn't embedded");
}
