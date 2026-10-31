//! Small hand-written PDFs for tests: Letter pages of Helvetica text (a
//! non-embedded standard font), optionally with an outline.

/// A PDF with one Letter page per entry of `pages`, each page's text at
/// (72, 700) in 12 pt Helvetica, lines 20 pt apart.
pub fn text_pdf(pages: &[&[&str]]) -> Vec<u8> {
    build(pages, None)
}

/// Three pages and an outline: "Intro" (an explicit `/Dest` to page 1) with a
/// child "Details" (a `/GoTo` action to page 3), then "Named" (a named
/// destination, resolved through the catalog's `/Names /Dests` tree, to page 2).
pub fn outlined_pdf() -> Vec<u8> {
    build(&[&["One"], &["Two"], &["Three"]], Some(()))
}

fn build(pages: &[&[&str]], outline: Option<()>) -> Vec<u8> {
    // Objects: 1 catalog, 2 pages, 3 font, then per page (page, content), then outline objects.
    let first_page = 4;
    let page_ids: Vec<usize> = (0..pages.len()).map(|i| first_page + 2 * i).collect();
    let after_pages = first_page + 2 * pages.len();
    let mut objects: Vec<String> = Vec::new();
    let catalog_extra = if outline.is_some() {
        format!(
            "/Outlines {} 0 R /Names << /Dests << /Names [(chap2) [{} 0 R /Fit]] >> >>",
            after_pages, page_ids[1]
        )
    } else {
        String::new()
    };
    objects.push(format!("<< /Type /Catalog /Pages 2 0 R {catalog_extra} >>"));
    let kids: Vec<String> = page_ids.iter().map(|id| format!("{id} 0 R")).collect();
    objects.push(format!(
        "<< /Type /Pages /Kids [{}] /Count {} >>",
        kids.join(" "),
        pages.len()
    ));
    objects.push(
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".into(),
    );
    for (i, lines) in pages.iter().enumerate() {
        let mut content = String::from("BT /F1 12 Tf 72 700 Td");
        for (n, line) in lines.iter().enumerate() {
            if n > 0 {
                content.push_str(" 0 -20 Td");
            }
            content.push_str(&format!(" ({}) Tj", escape(line)));
        }
        content.push_str(" ET");
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>",
            page_ids[i] + 1
        ));
        objects.push(format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len() + 1
        ));
    }
    if outline.is_some() {
        let (root, intro, details, named) = (
            after_pages,
            after_pages + 1,
            after_pages + 2,
            after_pages + 3,
        );
        objects.push(format!(
            "<< /Type /Outlines /First {intro} 0 R /Last {named} 0 R /Count 3 >>"
        ));
        objects.push(format!(
            "<< /Title (Intro) /Parent {root} 0 R /Next {named} 0 R /First {details} 0 R /Last {details} 0 R /Count 1 /Dest [{} 0 R /XYZ 0 792 0] >>",
            page_ids[0]
        ));
        objects.push(format!(
            "<< /Title <FEFF00440065007400610069006C0073> /Parent {intro} 0 R /A << /S /GoTo /D [{} 0 R /Fit] >> >>",
            page_ids[2]
        ));
        objects.push(format!(
            "<< /Title (Named) /Parent {root} 0 R /Prev {intro} 0 R /Dest (chap2) >>"
        ));
    }

    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)")
}
