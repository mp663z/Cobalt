use kobo_web_document::{parse_style_tree, style_diagnostics::RefusalReason as R, Limits};
fn tree(css: &str) -> kobo_web_document::style_tree::StyleTree {
    parse_style_tree(
        format!("<!doctype html><style>{css}</style><p>x</p>").as_bytes(),
        &[],
        &Limits::DEFAULT,
    )
}
fn color(css: &str, expected: u32) {
    let t = tree(css);
    assert!(!t.unsupported, "{css}: {:?}", t.diagnostics);
    assert_eq!(
        t.nodes.iter().find(|n| n.tag == "p").unwrap().style.color,
        expected
    );
}
#[test]
fn charset_exact_prefix_only() {
    for css in [
        "@charset \"utf-8\";p{color:red}",
        "@charset \"UTF-8\";p{color:red}",
        "\u{feff}@charset \"uTf-8\";p{color:red}",
        "\u{feff}p{color:red}",
    ] {
        color(css, 0x00ff_0000);
    }
    for prefix in [
        " @charset \"utf-8\";",
        "/*x*/@charset \"utf-8\";",
        "@CHARSET \"utf-8\";",
        "@charset  \"utf-8\";",
        "@charset 'utf-8';",
        "@charset \"utf-8\" ;",
        "@charset \"utf8\";",
        "@charset \"windows-1252\";",
        "p{}@charset \"utf-8\";",
        "@charset \"utf-8\";@charset \"utf-8\";",
        "@media screen{@charset \"utf-8\";}",
        "\u{feff}@charset \"utf-16\";",
    ] {
        let t = tree(&format!("{prefix}p{{color:red}}"));
        assert!(t.unsupported, "{prefix}");
        assert!(t.diagnostics.count(R::CssAtRule) > 0, "{prefix}");
    }
}
#[test]
fn media_lists_nested_and_cascade() {
    for media in [
        "screen",
        "all",
        "only screen",
        "ONLY ALL",
        "print, screen",
        "only print, only screen",
        "",
    ] {
        color(&format!("@media {media}{{p{{color:red}}}}"), 0x00ff_0000);
    }
    color("p{color:red}@media only print{p{color:blue}}", 0x00ff_0000);
    color("@media screen{@media all{p{color:red}}}", 0x00ff_0000);
    color(
        "p{color:red}@media screen{@media print{p{color:blue}}}",
        0x00ff_0000,
    );
    color("@media screen{p{color:red}}p{color:blue}", 255);
    color("p{color:blue}@media screen{p{color:red}}", 0x00ff_0000);
    color(
        "@media print{p{display:grid;unknown:thing}}p{color:red}",
        0x00ff_0000,
    );
}
#[test]
fn unknown_dominates_true_false_and_empty_groups() {
    for css in [
        "@media screen, (width:1px){}",
        "@media print{@media (width:1px){}}",
        "@media print{@supports(x:y){p{color:red}}}",
        "@layer{}",
        "@media not print{}",
        "@media unknown{}",
        "@media screen,{}",
        "@media s/**/creen{}",
        "@media only{}",
        "@media \u{a0}{}",
        "@media screen\u{a0}{}",
        "@media screen and (width:1px){p{color:red}}",
    ] {
        let t = tree(css);
        assert!(t.unsupported, "{css}");
        assert!(t.diagnostics.count(R::CssCondition) > 0, "{css}");
    }
}
#[test]
fn inactive_structure_and_work_still_refuse() {
    for css in [
        "@media print{@import 'x';}",
        "@media print{p{color:red}",
        "@media print{@unknown{p{color:red}}}",
    ] {
        assert!(tree(css).unsupported, "{css}");
    }
    let nested = format!(
        "{}p{{color:red}}{}",
        "@media print{".repeat(17),
        "}".repeat(17)
    );
    assert!(tree(&nested).diagnostics.count(R::CssScanBudget) > 0);
    let many = format!("@media print{{{}}}", "p{}".repeat(4097));
    assert!(tree(&many).diagnostics.count(R::CssScanBudget) > 0);
}
#[test]
fn charset_and_media_reachable_via_retained_data() {
    let html=b"<!doctype html><link rel=stylesheet href='data:text/css;base64,QGNoYXJzZXQgInV0Zi04IjtAbWVkaWEgb25seSBzY3JlZW57cHtjb2xvcjpyZWR9fQ=='><p>x</p>";
    let t = parse_style_tree(html, &[], &Limits::DEFAULT);
    assert!(!t.unsupported, "{:?}", t.diagnostics);
    assert_eq!(
        t.nodes.iter().find(|n| n.tag == "p").unwrap().style.color,
        0x00ff_0000
    );
}
