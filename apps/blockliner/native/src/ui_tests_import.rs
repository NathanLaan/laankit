//! Opening a hand-written flowchart, headlessly: its nodes are laid out in
//! ranks, shapes BlockLiner can't fully edit say why, and the first edit
//! saves the layout with the kept statements intact.

use std::fs;

use egui::Key;

use crate::ui_tests::{at, click, harness};

const HANDWRITTEN: &str = "\
flowchart TD
  web[Web App] --> api(API Gateway)
  api --> orders
  api --> bus[(Event Bus)]
  subgraph payments [Payments]
    pay[Payment Service] --> psp
  end
  orders --> pay
";

#[test]
fn a_hand_written_file_is_laid_out_in_ranks() {
    let (_tmp, paths, mut h) = harness();
    let folder = paths.prefs().last_project.unwrap();
    let path = folder.join("Diagram.mmd");
    fs::write(&path, HANDWRITTEN).unwrap();
    // Reopen the folder to read it.
    h.state_mut().open_project(folder, None);
    h.run_steps(2);

    let d = h.state().diagram();
    let y = |id: &str| d.shape(id).unwrap().rect.y;
    // Top to bottom, one rank per step of the flow; the subgraph's own
    // edge (pay → psp) ranks psp too.
    assert!(y("web") < y("api"));
    assert!(y("api") < y("orders"));
    assert_eq!(y("orders"), y("bus"));
    assert!(y("orders") < y("pay"));
    assert!(y("pay") < y("psp"));
    let rect = |id: &str| d.shape(id).unwrap().rect;
    assert_eq!(rect("web").center().x, rect("api").center().x, "a column");

    // The Event Bus is a Mermaid shape drawn as a box; psp is declared in
    // the subgraph. Hovering says so; a plain box says nothing.
    let note = |h: &crate::ui_tests::H| {
        let s = h.state();
        s.canvas().limitation(s.diagram()).map(|(_, text)| text)
    };
    let center = |id: &str| {
        let c = rect(id).center();
        at(c.x, c.y)
    };
    let (bus, psp, web) = (center("bus"), center("psp"), center("web"));
    h.hover_at(bus);
    h.run_steps(2);
    assert!(note(&h).unwrap().contains("[( )] shape"), "{:?}", note(&h));
    h.hover_at(psp);
    h.run_steps(2);
    assert!(note(&h).unwrap().contains("not renamed or deleted"));
    h.hover_at(web);
    h.run_steps(2);
    assert_eq!(note(&h), None);

    // Nothing is written until an edit; then the layout is, around the
    // statements kept as they were.
    assert!(!h.state().is_saving());
    assert_eq!(fs::read_to_string(&path).unwrap(), HANDWRITTEN);
    click(&mut h, web);
    h.key_press(Key::ArrowRight);
    h.run_steps(2);
    h.state().flush();
    let text = fs::read_to_string(&path).unwrap();
    for kept in [
        "  subgraph payments [Payments]\n",
        "    pay[Payment Service] --> psp\n",
        "  end\n",
        "%% @shape psp ",
        "%% @shape bus ",
    ] {
        assert!(text.contains(kept), "{kept:?} in\n{text}");
    }
}
