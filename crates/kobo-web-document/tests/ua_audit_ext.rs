//! PRIVATE candidate (ext entry only): UA presentation audit. Chrome 156 (unforced, light scheme, 16px parent; probe in
//! private-evidence ua1) gives these elements presentation the engine never modelled, and the base admitted them as plain
//! text. Under ext each listed element needs an AUTHOR declaration for every affected property, else the page refuses.
//! Style-layer admission only (`unsupported`); no pixel claim. Default entry is UNCHANGED (still admits them).
use kobo_web_document::{parse_style_tree, parse_style_tree_with_inline_sheets_and_selectors, Limits, SupplyBudget};
fn ext(css: &str, body: &str) -> kobo_web_document::style_tree::StyleTree {
    let h = format!("<!doctype html><style>{css}</style><div>{body}</div>");
    parse_style_tree_with_inline_sheets_and_selectors(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL)
}
fn dflt(body: &str) -> kobo_web_document::style_tree::StyleTree {
    let h = format!("<!doctype html><div>{body}</div>");
    parse_style_tree(h.as_bytes(), &[], &Limits::DEFAULT)
}
/// (tag, markup, full author css that sets every property Chrome's UA sets for it, list of single-property css
/// strings, each of which leaves one required property out).
fn cases() -> Vec<(&'static str, &'static str, &'static str, Vec<&'static str>)> {
    vec![
        ("h1", "<h1>t</h1>", "h1{font-size:32px;font-weight:700;margin:0}", vec!["h1{font-weight:700;margin:0}", "h1{font-size:32px;margin:0}", "h1{font-size:32px;font-weight:700;margin-bottom:0}", "h1{font-size:32px;font-weight:700;margin-top:0}"]),
        ("h2", "<h2>t</h2>", "h2{font-size:24px;font-weight:700;margin:0}", vec!["h2{font-weight:700;margin:0}"]),
        ("h3", "<h3>t</h3>", "h3{font-size:18px;font-weight:700;margin:0}", vec!["h3{font-size:18px;margin:0}"]),
        ("h4", "<h4>t</h4>", "h4{font-weight:700;margin:0}", vec!["h4{margin:0}", "h4{font-weight:700}"]),
        ("h5", "<h5>t</h5>", "h5{font-size:13px;font-weight:700;margin:0}", vec!["h5{font-weight:700;margin:0}"]),
        ("h6", "<h6>t</h6>", "h6{font-size:10px;font-weight:700;margin:0}", vec!["h6{font-size:10px;margin:0}"]),
        ("blockquote", "<blockquote>t</blockquote>", "blockquote{margin:0}", vec!["blockquote{margin-top:0;margin-bottom:0;margin-left:0}", "blockquote{margin-top:0;margin-bottom:0;margin-right:0}"]),
        ("figure", "<figure>t</figure>", "figure{margin:0}", vec!["figure{margin-top:0;margin-bottom:0;margin-left:0}"]),
        ("small", "<small>t</small>", "small{font-size:13px}", vec![]),
        ("big", "<big>t</big>", "big{font-size:19px}", vec![]),
        ("ul", "<ul><li>t</li></ul>", "ul{margin:0;padding:0;list-style-type:none}", vec!["ul{margin:0;list-style-type:none}", "ul{padding:0;list-style-type:none}", "ul{margin-top:0;padding:0;list-style-type:none}"]),
        ("ol", "<ol><li>t</li></ol>", "ol{margin:0;padding:0;list-style-type:none}", vec!["ol{margin:0;list-style-type:none}", "ol{padding:0;list-style-type:none}"]),
        ("address", "<address>t</address>", "address{display:block;font-style:normal}", vec!["address{display:block}", "address{font-style:normal}"]),
        ("cite", "<cite>t</cite>", "cite{font-style:normal}", vec![]),
        ("dfn", "<dfn>t</dfn>", "dfn{font-style:normal}", vec![]),
        ("var", "<var>t</var>", "var{font-style:normal}", vec![]),
    ]
}
#[test]
fn bare_listed_elements_refuse_under_ext_and_are_admitted_by_default() {
    for (tag, body, _, _) in cases().into_iter().chain([("code", "<code>t</code>", "", vec![]), ("samp", "<samp>t</samp>", "", vec![]), ("kbd", "<kbd>t</kbd>", "", vec![]), ("tt", "<tt>t</tt>", "", vec![])]) {
        assert!(ext("", body).unsupported, "ext must refuse bare <{tag}>");
        assert!(!dflt(body).unsupported, "anchor: default entry admits bare <{tag}> (unchanged, pre-existing gap)");
    }
}
#[test]
fn fully_authored_elements_pass_the_audit_and_each_missing_property_refuses() {
    for (tag, body, full, missing) in cases() {
        let t = ext(full, body);
        assert!(!t.unsupported, "<{tag}> with every property authored: {full}");
        for m in missing {
            assert!(ext(m, body).unsupported, "<{tag}> missing a property must refuse: {m}");
        }
    }
}
#[test]
fn inline_style_counts_as_author_and_unlisted_elements_are_untouched() {
    let t = ext("", "<h4 style='font-weight:700;margin:0'>t</h4>");
    assert!(!t.unsupported, "inline author declarations satisfy the audit");
    assert!(ext("", "<h4 style='font-weight:700'>t</h4>").unsupported);
    // unlisted elements keep their previous behaviour
    for body in ["<p>t</p>", "<span>t</span>", "<b>t</b>", "<section>t</section>", "<nav>t</nav>"] {
        assert!(!ext("", body).unsupported, "{body}");
    }
}
#[test]
fn unconditional_refusals_stay_refused_even_when_author_sets_everything() {
    for (css, body) in [
        ("pre{margin:0;font-family:serif;white-space:normal}", "<pre>t</pre>"),
        ("sub{font-size:13px}", "<sub>t</sub>"),
        ("sup{font-size:13px}", "<sup>t</sup>"),
        ("button{display:inline-block;font-size:13px;padding:0;font-family:serif;text-align:center;background-color:#eee}", "<button>t</button>"),
        ("textarea{display:inline-block;font-size:13px;padding:0;font-family:serif}", "<textarea>t</textarea>"),
        ("select{display:inline-block;font-size:13px;font-family:serif}", "<select><option>t</option></select>"),
        ("details{display:block}", "<details><summary>s</summary>t</details>"),
        ("fieldset{margin:0;padding:0}", "<fieldset>t</fieldset>"),
        ("legend{display:block;padding:0}", "<fieldset style='margin:0;padding:0'><legend>t</legend></fieldset>"),
        // monospace UA family: an author font-family is itself refused by the font lane, so these stay refused
        ("code{font-family:serif}", "<code>t</code>"),
        ("samp{font-family:serif}", "<samp>t</samp>"),
        ("kbd{font-family:serif}", "<kbd>t</kbd>"),
        ("tt{font-family:serif}", "<tt>t</tt>"),
        ("center{display:block;text-align:center}", "<center>t</center>"),
    ] {
        assert!(ext(css, body).unsupported, "{body}");
    }
}
#[test]
fn unconditional_tags_refuse_on_their_own_not_only_through_a_refused_author_value() {
    // Minimal author css with only supported values, so a refusal can only come from the audit.
    for (css, body) in [
        ("pre{margin:0}", "<pre>t</pre>"),
        ("sub{font-weight:400}", "<sub>t</sub>"),
        ("sup{font-weight:400}", "<sup>t</sup>"),
        ("button{margin:0}", "<button>t</button>"),
        ("textarea{margin:0}", "<textarea>t</textarea>"),
        ("select{margin:0}", "<select><option>t</option></select>"),
        ("details{display:block}", "<details>t</details>"),
        ("summary{display:block}", "<details style='display:block'><summary>t</summary></details>"),
        ("fieldset{margin:0}", "<fieldset>t</fieldset>"),
        ("legend{display:block}", "<fieldset style='margin:0'><legend>t</legend></fieldset>"),
        ("center{display:block}", "<center>t</center>"),
    ] {
        assert!(ext(css, body).unsupported, "{body}");
    }
}
#[test]
fn author_revert_hands_back_to_the_unmodelled_ua_sheet_and_refuses() {
    // Chrome 156 (private-evidence ua1/revert): h1 revert -> 32px/700/21.44px; unset|initial|inherit -> 16px/400/0.
    for (css, body) in [
        ("h1{font-size:revert;font-weight:revert;margin:revert}", "<h1>t</h1>"),
        ("h1{font-size:32px;font-weight:700;margin:revert}", "<h1>t</h1>"),
        ("h1{font-size:32px;font-weight:revert;margin:0}", "<h1>t</h1>"),
        ("h1{font-size:revert;font-weight:700;margin:0}", "<h1>t</h1>"),
        ("h1{font-size:32px;font-weight:700;margin-top:revert;margin-bottom:0}", "<h1>t</h1>"),
        ("h1{margin:0;font-size:32px;font-weight:700}h1{margin:revert}", "<h1>t</h1>"),
        ("ul{margin:revert;padding:revert;list-style:none}", "<ul><li>t</li></ul>"),
        ("ul{margin:0;padding:revert;list-style:none}", "<ul><li>t</li></ul>"),
        ("ul{margin:0;padding:0;list-style:none}ul{padding-left:revert}", "<ul><li>t</li></ul>"),
        ("blockquote{margin:revert}", "<blockquote>t</blockquote>"),
        ("address{display:block;font-style:revert}", "<address>t</address>"),
        ("small{font-size:revert}", "<small>t</small>"),
    ] {
        assert!(ext(css, body).unsupported, "{css}");
    }
    // anchors: the same pages with real author replacements are admitted
    assert!(!ext("h1{font-size:32px;font-weight:700;margin:0}", "<h1>t</h1>").unsupported);
    assert!(!ext("ul{margin:0;padding:0;list-style:none}", "<ul><li>t</li></ul>").unsupported);
    // inline style revert refuses too
    assert!(ext("", "<h4 style='font-weight:700;margin:revert'>t</h4>").unsupported);
}
#[test]
fn author_initial_inherit_unset_are_replacements_matching_chrome() {
    for css in ["h4{font-weight:unset;margin:unset}", "h4{font-weight:initial;margin:initial}", "h4{font-weight:inherit;margin:inherit}"] {
        let t = ext(css, "<h4>t</h4>");
        // Chrome computes 400/0px here (probe); admitted when the engine supports the keyword, refused otherwise: never UA 700/21.28px.
        if !t.unsupported {
            let n = t.nodes.iter().find(|n| n.tag == "h4").unwrap();
            assert_eq!((n.style.font_weight, n.style.margin_top), (400, kobo_web_document::computed_style::Margin::Px(0)), "{css}");
        }
    }
}
#[test]
fn summary_and_legend_refuse_on_their_own_with_no_details_or_fieldset_in_the_markup() {
    // Isolating markup (no unconditional details/fieldset ancestor), anchored against an unlisted tag with the same css.
    assert!(!ext("section{display:block}", "<section>t</section>").unsupported);
    assert!(ext("summary{display:block}", "<summary>t</summary>").unsupported);
    assert!(ext("legend{display:block;padding:0}", "<legend>t</legend>").unsupported);
    assert!(!ext("summary{display:block}", "<span>t</span>").unsupported);
}
#[test]
fn var_resolved_revert_and_var_resolved_replacement_are_judged_after_substitution() {
    // var() substituted to `revert`: Chrome 156 treats it as invalid-at-computed-time = unset (0px/16px/400, probe
    // private-evidence ua1/rv2), NOT the UA sheet. The engine refuses conservatively (over-refusal, safe; not a Chrome match).
    assert!(ext("h1{--r:revert;font-size:32px;font-weight:700;margin:var(--r)}", "<h1>t</h1>").unsupported);
    assert!(ext("h1{--r:revert;font-size:var(--r);font-weight:700;margin:0}", "<h1>t</h1>").unsupported);
    // var resolving to a real replacement counts as authored (previously over-refused).
    assert!(!ext("h1{--z:0;--s:32px;--w:700;font-size:var(--s);font-weight:var(--w);margin:var(--z)}", "<h1>t</h1>").unsupported);
    // var resolving to a replacement for only some required properties still refuses the rest.
    assert!(ext("h1{--z:0;margin:var(--z);font-weight:700}", "<h1>t</h1>").unsupported);
}
#[test]
fn table_family_and_other_unmodelled_ua_elements_refuse_under_ext_each_in_isolation() {
    // Chrome 156 (private-evidence ua2/tb.json, unforced light): table border-spacing 2px, th 700/center, td/th padding 1px,
    // caption center, hr 8px margins + 1px inset border + grey, input 13.33px inset border 177px wide, iframe 300x150 + inset
    // border, ruby/rt (8px) and q (quote marks). The engine's UA sheet makes all of them plain inline (`ua_display`), so the
    // default entry admits them. The non-table cases are single-tag markup. Table-family markup needs its parser-required
    // wrappers (table/tr/td are all refused), so those cases prove family refusal, NOT per-tag causality; the mutant check below
    // removes the whole family together.
    for (tag, body) in [
        ("hr", "<hr>"),
        ("input", "<input>"),
        ("iframe", "<iframe></iframe>"),
        ("q", "<q>x</q>"),
        ("ruby", "<ruby>r</ruby>"),
        ("rt", "<ruby>r<rt>t</rt></ruby>"),
        ("table", "<table></table>"),
        ("caption", "<table><caption>c</caption></table>"),
        ("colgroup", "<table><colgroup></colgroup></table>"),
        ("tr", "<table><tr><td>x</td></tr></table>"),
        ("td", "<table><tr><td>x</td></tr></table>"),
        ("th", "<table><tr><th>x</th></tr></table>"),
        ("thead", "<table><thead><tr><td>x</td></tr></thead></table>"),
        ("tbody", "<table><tbody><tr><td>x</td></tr></tbody></table>"),
        ("tfoot", "<table><tfoot><tr><td>x</td></tr></tfoot></table>"),
    ] {
        assert!(ext("", &format!("<div>{body}</div>")).nodes.iter().any(|n| n.tag == tag), "{tag}: parser kept the tag in this markup");
        assert!(ext("", &format!("<div>{body}</div>")).unsupported, "ext refuses {tag}");
        assert!(!dflt(&format!("<div>{body}</div>")).unsupported, "default entry still admits {tag} (unchanged)");
    }
    // anchor: an unlisted inline element with the same wrapper is admitted
    assert!(!ext("", "<div><span>x</span></div>").unsupported);
}
#[test]
fn replaced_media_and_hgroup_refuse_under_ext_each_in_isolation() {
    // Chrome 156 (private-evidence ua2/me.json): video/canvas/object 300x150 replaced boxes, audio display:none, svg/img sized
    // replaced content, hgroup display:block; the engine UA sheet makes all of them plain Inline (ua2/engine3.txt), admitted
    // by both entries. hgroup wraps NO heading here so a refusal cannot come from a bare h1-h6.
    for (tag, body) in [
        ("img", "<img alt=a width=10 height=10>"),
        ("svg", "<svg width=10 height=10></svg>"),
        ("video", "<video></video>"),
        ("audio", "<audio></audio>"),
        ("canvas", "<canvas></canvas>"),
        ("object", "<object></object>"),
        ("embed", "<embed>"),
        ("hgroup", "<hgroup>x</hgroup>"),
    ] {
        let wrapped = format!("<div>{body}</div>");
        assert!(ext("", &wrapped).nodes.iter().any(|n| n.tag == tag), "{tag}: parser kept the tag");
        assert!(ext("", &wrapped).unsupported, "ext refuses {tag}");
        assert!(!dflt(&wrapped).unsupported, "default entry still admits {tag} (unchanged)");
    }
    // anchors: unlisted inline/block neighbours with the same wrapper stay admitted (label, br, picture, nav)
    for body in ["<label>l</label>", "a<br>b", "<nav>n</nav>"] {
        assert!(!ext("", &format!("<div>{body}</div>")).unsupported, "{body}");
    }
}
