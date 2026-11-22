//! The package: escaping, relationships, and the parts every `.pptx` has
//! whatever is on its slides.

pub type Rgb = [u8; 3];

pub const NS: &str = "xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
    xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
    xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\"";
pub const DECL: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";

const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
pub const REL_HYPERLINK: &str = "hyperlink";
pub const REL_IMAGE: &str = "image";
pub const REL_LAYOUT: &str = "slideLayout";
pub const REL_MASTER: &str = "slideMaster";
pub const REL_NOTES: &str = "notesSlide";
pub const REL_NOTES_MASTER: &str = "notesMaster";
pub const REL_SLIDE: &str = "slide";
pub const REL_THEME: &str = "theme";
pub const REL_TABLE_STYLES: &str = "tableStyles";

/// EMU in a stage pixel: 1920 stage pixels are PowerPoint's 13.333 inches.
pub const EMU_PER_PX: f32 = 6350.0;

pub fn emu(px: f32) -> i64 {
    if px.is_finite() {
        (px * EMU_PER_PX).round() as i64
    } else {
        0
    }
}

pub fn hex(c: Rgb) -> String {
    format!("{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

/// Text for an element or an attribute. Characters XML 1.0 cannot hold are
/// dropped.
pub fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' => out.push_str("&#9;"),
            '\n' | '\r' => out.push(' '),
            c if (c as u32) < 0x20 => {}
            '\u{FFFE}' | '\u{FFFF}' => {}
            c => out.push(c),
        }
    }
    out
}

/// One part's relationships.
#[derive(Default)]
pub struct Rels {
    items: Vec<(&'static str, String, bool)>,
}

impl Rels {
    fn add(&mut self, kind: &'static str, target: &str, external: bool) -> String {
        let at = self
            .items
            .iter()
            .position(|i| i.0 == kind && i.1 == target && i.2 == external)
            .unwrap_or_else(|| {
                self.items.push((kind, target.to_string(), external));
                self.items.len() - 1
            });
        format!("rId{}", at + 1)
    }

    /// A part in the package, by its path from this part's folder.
    pub fn part(&mut self, kind: &'static str, target: &str) -> String {
        self.add(kind, target, false)
    }

    /// An address outside the file.
    pub fn external(&mut self, kind: &'static str, target: &str) -> String {
        self.add(kind, target, true)
    }

    pub fn xml(&self) -> String {
        let mut s = format!(
            "{DECL}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">"
        );
        for (i, (kind, target, external)) in self.items.iter().enumerate() {
            s.push_str(&format!(
                "<Relationship Id=\"rId{}\" Type=\"{REL}/{kind}\" Target=\"{}\"{}/>",
                i + 1,
                esc(target),
                if *external {
                    " TargetMode=\"External\""
                } else {
                    ""
                }
            ));
        }
        s.push_str("</Relationships>");
        s
    }
}

/// The colours a file's theme offers in PowerPoint's colour pickers.
pub struct Palette {
    pub bg: Rgb,
    pub fg: Rgb,
    pub muted: Rgb,
    pub accent: Rgb,
    pub rule: Rgb,
    pub code_bg: Rgb,
}

impl Palette {
    pub fn light() -> Palette {
        Palette {
            bg: [255, 255, 255],
            fg: [0, 0, 0],
            muted: [68, 84, 106],
            accent: [68, 114, 196],
            rule: [165, 165, 165],
            code_bg: [231, 230, 230],
        }
    }

    fn dark(&self) -> bool {
        let lum = |c: Rgb| 0.299 * c[0] as f32 + 0.587 * c[1] as f32 + 0.114 * c[2] as f32;
        lum(self.bg) < lum(self.fg)
    }

    /// PowerPoint's colour map: which theme colours are the background and
    /// the text. A dark deck's background is the theme's dark colour.
    pub fn color_map(&self) -> &'static str {
        if self.dark() {
            "bg1=\"dk1\" tx1=\"lt1\" bg2=\"dk2\" tx2=\"lt2\" accent1=\"accent1\" accent2=\"accent2\" accent3=\"accent3\" accent4=\"accent4\" accent5=\"accent5\" accent6=\"accent6\" hlink=\"hlink\" folHlink=\"folHlink\""
        } else {
            "bg1=\"lt1\" tx1=\"dk1\" bg2=\"lt2\" tx2=\"dk2\" accent1=\"accent1\" accent2=\"accent2\" accent3=\"accent3\" accent4=\"accent4\" accent5=\"accent5\" accent6=\"accent6\" hlink=\"hlink\" folHlink=\"folHlink\""
        }
    }
}

pub fn theme(name: &str, p: &Palette, text_font: &str, code_font: &str) -> String {
    let c = |tag: &str, v: Rgb| format!("<a:{tag}><a:srgbClr val=\"{}\"/></a:{tag}>", hex(v));
    let (dk1, lt1, dk2, lt2) = if p.dark() {
        (p.bg, p.fg, p.code_bg, p.muted)
    } else {
        (p.fg, p.bg, p.muted, p.code_bg)
    };
    let font = |tag: &str, face: &str| {
        format!(
            "<a:{tag}><a:latin typeface=\"{}\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:{tag}>",
            esc(face)
        )
    };
    let _ = code_font;
    let fill = "<a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill>";
    let line = |w: u32| {
        format!("<a:ln w=\"{w}\" cap=\"flat\" cmpd=\"sng\" algn=\"ctr\">{fill}<a:prstDash val=\"solid\"/></a:ln>")
    };
    let effect = "<a:effectStyle><a:effectLst/></a:effectStyle>";
    format!(
        "{DECL}<a:theme xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" name=\"{name}\"><a:themeElements>\
<a:clrScheme name=\"{name}\">{}{}{}{}{}{}{}{}{}{}{}{}</a:clrScheme>\
<a:fontScheme name=\"{name}\">{}{}</a:fontScheme>\
<a:fmtScheme name=\"{name}\"><a:fillStyleLst>{fill}{fill}{fill}</a:fillStyleLst>\
<a:lnStyleLst>{}{}{}</a:lnStyleLst>\
<a:effectStyleLst>{effect}{effect}{effect}</a:effectStyleLst>\
<a:bgFillStyleLst>{fill}{fill}{fill}</a:bgFillStyleLst></a:fmtScheme>\
</a:themeElements><a:objectDefaults/><a:extraClrSchemeLst/></a:theme>",
        c("dk1", dk1),
        c("lt1", lt1),
        c("dk2", dk2),
        c("lt2", lt2),
        c("accent1", p.accent),
        c("accent2", p.muted),
        c("accent3", p.rule),
        c("accent4", p.code_bg),
        c("accent5", p.fg),
        c("accent6", p.bg),
        c("hlink", p.accent),
        c("folHlink", p.muted),
        font("majorFont", text_font),
        font("minorFont", text_font),
        line(6350),
        line(12700),
        line(19050),
        name = esc(name),
    )
}

/// The empty group every shape tree starts with.
pub const TREE_HEAD: &str = "<p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>\
<p:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/><a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"0\" cy=\"0\"/></a:xfrm></p:grpSpPr>";

fn placeholder(id: u32, name: &str, ph: &str, x: i64, y: i64, w: i64, h: i64) -> String {
    format!(
        "<p:sp><p:nvSpPr><p:cNvPr id=\"{id}\" name=\"{name}\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr><p:ph {ph}/></p:nvPr></p:nvSpPr>\
<p:spPr><a:xfrm><a:off x=\"{x}\" y=\"{y}\"/><a:ext cx=\"{w}\" cy=\"{h}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></p:spPr>\
<p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang=\"en-US\"/></a:p></p:txBody></p:sp>"
    )
}

fn level_style(size: u32, color: &str, bold: bool) -> String {
    format!(
        "<a:lvl1pPr marL=\"0\" indent=\"0\" algn=\"l\"><a:buNone/><a:defRPr sz=\"{size}\" b=\"{}\"><a:solidFill><a:schemeClr val=\"{color}\"/></a:solidFill><a:latin typeface=\"+mn-lt\"/><a:ea typeface=\"+mn-ea\"/><a:cs typeface=\"+mn-cs\"/></a:defRPr></a:lvl1pPr>",
        bold as u8
    )
}

/// The slide master: the deck's background, and where a title and a body go
/// on a slide added in PowerPoint.
pub fn slide_master(p: &Palette, w: i64, h: i64) -> String {
    let pad_x = emu(noteliner_slides::render::PAD_X);
    let pad_y = emu(noteliner_slides::render::PAD_Y);
    let title_h = emu(184.0);
    let body_y = pad_y + title_h + emu(noteliner_slides::render::GAP);
    format!(
        "{DECL}<p:sldMaster {NS}><p:cSld><p:bg><p:bgPr><a:solidFill><a:srgbClr val=\"{}\"/></a:solidFill><a:effectLst/></p:bgPr></p:bg>\
<p:spTree>{TREE_HEAD}{}{}</p:spTree></p:cSld>\
<p:clrMap {}/>\
<p:sldLayoutIdLst><p:sldLayoutId id=\"2147483649\" r:id=\"rId1\"/></p:sldLayoutIdLst>\
<p:txStyles><p:titleStyle>{}</p:titleStyle><p:bodyStyle>{}</p:bodyStyle><p:otherStyle>{}</p:otherStyle></p:txStyles></p:sldMaster>",
        hex(p.bg),
        placeholder(2, "Title Placeholder 1", "type=\"title\"", pad_x, pad_y, w - 2 * pad_x, title_h),
        placeholder(3, "Text Placeholder 2", "type=\"body\" idx=\"1\"", pad_x, body_y, w - 2 * pad_x, (h - body_y - pad_y).max(0)),
        p.color_map(),
        level_style(5200, "accent1", true),
        level_style(2200, "tx1", false),
        level_style(1800, "tx1", false),
    )
}

/// The one layout: a title, and nothing else.
pub fn slide_layout(w: i64) -> String {
    let pad_x = emu(noteliner_slides::render::PAD_X);
    let pad_y = emu(noteliner_slides::render::PAD_Y);
    format!(
        "{DECL}<p:sldLayout {NS} type=\"titleOnly\" preserve=\"1\"><p:cSld name=\"Title Only\"><p:spTree>{TREE_HEAD}{}</p:spTree></p:cSld>\
<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>",
        placeholder(2, "Title 1", "type=\"title\"", pad_x, pad_y, w - 2 * pad_x, emu(184.0)),
    )
}

/// The notes master: a notes page is a picture of the slide above its notes.
pub fn notes_master() -> String {
    format!(
        "{DECL}<p:notesMaster {NS}><p:cSld><p:bg><p:bgPr><a:solidFill><a:srgbClr val=\"FFFFFF\"/></a:solidFill><a:effectLst/></p:bgPr></p:bg>\
<p:spTree>{TREE_HEAD}\
<p:sp><p:nvSpPr><p:cNvPr id=\"2\" name=\"Slide Image Placeholder 1\"/><p:cNvSpPr><a:spLocks noGrp=\"1\" noRot=\"1\" noChangeAspect=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"sldImg\" idx=\"2\"/></p:nvPr></p:nvSpPr>\
<p:spPr><a:xfrm><a:off x=\"685800\" y=\"1143000\"/><a:ext cx=\"5486400\" cy=\"3086100\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom><a:noFill/><a:ln w=\"12700\"><a:solidFill><a:srgbClr val=\"000000\"/></a:solidFill></a:ln></p:spPr></p:sp>\
<p:sp><p:nvSpPr><p:cNvPr id=\"3\" name=\"Notes Placeholder 2\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"body\" sz=\"quarter\" idx=\"3\"/></p:nvPr></p:nvSpPr>\
<p:spPr><a:xfrm><a:off x=\"685800\" y=\"4400550\"/><a:ext cx=\"5486400\" cy=\"3600450\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></p:spPr>\
<p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang=\"en-US\"/></a:p></p:txBody></p:sp>\
</p:spTree></p:cSld><p:clrMap {}/>\
<p:notesStyle><a:lvl1pPr marL=\"0\" algn=\"l\"><a:defRPr sz=\"1200\"><a:solidFill><a:schemeClr val=\"tx1\"/></a:solidFill><a:latin typeface=\"+mn-lt\"/><a:ea typeface=\"+mn-ea\"/><a:cs typeface=\"+mn-cs\"/></a:defRPr></a:lvl1pPr></p:notesStyle></p:notesMaster>",
        Palette::light().color_map()
    )
}

/// A slide's notes page.
pub fn notes_slide(paragraphs: &[String]) -> String {
    let mut body = String::new();
    for p in paragraphs {
        if p.is_empty() {
            body.push_str("<a:p><a:endParaRPr lang=\"en-US\"/></a:p>");
        } else {
            body.push_str(&format!(
                "<a:p><a:r><a:rPr lang=\"en-US\" dirty=\"0\"/><a:t>{}</a:t></a:r></a:p>",
                esc(p)
            ));
        }
    }
    if body.is_empty() {
        body.push_str("<a:p><a:endParaRPr lang=\"en-US\"/></a:p>");
    }
    format!(
        "{DECL}<p:notes {NS}><p:cSld><p:spTree>{TREE_HEAD}\
<p:sp><p:nvSpPr><p:cNvPr id=\"2\" name=\"Slide Image Placeholder 1\"/><p:cNvSpPr><a:spLocks noGrp=\"1\" noRot=\"1\" noChangeAspect=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"sldImg\" idx=\"2\"/></p:nvPr></p:nvSpPr><p:spPr/></p:sp>\
<p:sp><p:nvSpPr><p:cNvPr id=\"3\" name=\"Notes Placeholder 2\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"body\" sz=\"quarter\" idx=\"3\"/></p:nvPr></p:nvSpPr><p:spPr/>\
<p:txBody><a:bodyPr/><a:lstStyle/>{body}</p:txBody></p:sp>\
</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:notes>"
    )
}

pub fn presentation(slides: usize, w: i64, h: i64, rels: &mut Rels) -> String {
    let master = rels.part(REL_MASTER, "slideMasters/slideMaster1.xml");
    let notes = rels.part(REL_NOTES_MASTER, "notesMasters/notesMaster1.xml");
    let mut ids = String::new();
    for i in 0..slides {
        let id = rels.part(REL_SLIDE, &format!("slides/slide{}.xml", i + 1));
        ids.push_str(&format!("<p:sldId id=\"{}\" r:id=\"{id}\"/>", 256 + i));
    }
    rels.part(REL_THEME, "theme/theme1.xml");
    rels.part(REL_TABLE_STYLES, "tableStyles.xml");
    let list = if ids.is_empty() {
        String::new()
    } else {
        format!("<p:sldIdLst>{ids}</p:sldIdLst>")
    };
    format!(
        "{DECL}<p:presentation {NS} saveSubsetFonts=\"1\"><p:sldMasterIdLst><p:sldMasterId id=\"2147483648\" r:id=\"{master}\"/></p:sldMasterIdLst>\
<p:notesMasterIdLst><p:notesMasterId r:id=\"{notes}\"/></p:notesMasterIdLst>{list}\
<p:sldSz cx=\"{w}\" cy=\"{h}\"/><p:notesSz cx=\"6858000\" cy=\"9144000\"/></p:presentation>"
    )
}

pub fn table_styles() -> String {
    format!("{DECL}<a:tblStyleLst xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" def=\"{{5C22544A-7EE6-4342-B048-85BB5CBC8040}}\"/>")
}

pub fn root_rels() -> String {
    format!(
        "{DECL}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"{REL}/officeDocument\" Target=\"ppt/presentation.xml\"/>\
<Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/>\
<Relationship Id=\"rId3\" Type=\"{REL}/extended-properties\" Target=\"docProps/app.xml\"/>\
</Relationships>"
    )
}

pub fn core_props(title: &str, created: Option<&str>) -> String {
    let dates = created
        .map(|d| {
            let d = esc(d);
            format!("<dcterms:created xsi:type=\"dcterms:W3CDTF\">{d}</dcterms:created><dcterms:modified xsi:type=\"dcterms:W3CDTF\">{d}</dcterms:modified>")
        })
        .unwrap_or_default();
    format!(
        "{DECL}<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" xmlns:dcmitype=\"http://purl.org/dc/dcmitype/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">\
<dc:title>{}</dc:title>{dates}</cp:coreProperties>",
        esc(title)
    )
}

pub fn app_props(slides: usize) -> String {
    format!(
        "{DECL}<Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\"><Application>NoteLiner</Application><Slides>{slides}</Slides></Properties>"
    )
}

/// `[Content_Types].xml` for the parts in `names`.
pub fn content_types(names: &[String]) -> String {
    const P: &str = "application/vnd.openxmlformats-officedocument.presentationml";
    let mut s = format!(
        "{DECL}<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
<Default Extension=\"xml\" ContentType=\"application/xml\"/>\
<Default Extension=\"png\" ContentType=\"image/png\"/>\
<Default Extension=\"jpeg\" ContentType=\"image/jpeg\"/>\
<Default Extension=\"gif\" ContentType=\"image/gif\"/>"
    );
    for name in names {
        let kind = if name == "ppt/presentation.xml" {
            format!("{P}.presentation.main+xml")
        } else if name.starts_with("ppt/slideMasters/") {
            format!("{P}.slideMaster+xml")
        } else if name.starts_with("ppt/slideLayouts/") {
            format!("{P}.slideLayout+xml")
        } else if name.starts_with("ppt/slides/") {
            format!("{P}.slide+xml")
        } else if name.starts_with("ppt/notesMasters/") {
            format!("{P}.notesMaster+xml")
        } else if name.starts_with("ppt/notesSlides/") {
            format!("{P}.notesSlide+xml")
        } else if name.starts_with("ppt/theme/") {
            "application/vnd.openxmlformats-officedocument.theme+xml".to_string()
        } else if name == "ppt/tableStyles.xml" {
            format!("{P}.tableStyles+xml")
        } else if name == "docProps/core.xml" {
            "application/vnd.openxmlformats-package.core-properties+xml".to_string()
        } else if name == "docProps/app.xml" {
            "application/vnd.openxmlformats-officedocument.extended-properties+xml".to_string()
        } else {
            continue;
        };
        if name.contains("/_rels/") {
            continue;
        }
        s.push_str(&format!(
            "<Override PartName=\"/{}\" ContentType=\"{kind}\"/>",
            esc(name)
        ));
    }
    s.push_str("</Types>");
    s
}
