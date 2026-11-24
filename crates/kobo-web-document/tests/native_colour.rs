use kobo_web_document::{parse_coloured_document,parse_document,Block,Inline,Limits,Url};
fn colours(content:&[Inline],out:&mut Vec<(String,[u8;3],bool)>) {
    for inline in content {match inline {
        Inline::ColouredText{text,foreground,code,..}=>out.push((text.clone(),*foreground,*code)),
        Inline::Emphasis(inner)|Inline::Strong(inner)|Inline::Inert(inner)=>colours(inner,out),
        Inline::Link{label,..}=>colours(label,out),_=>{}
    }}
}
#[test]
fn duplicate_words_nested_links_and_code_keep_same_dom_colours() {
    let html=br#"<!doctype html><style>p{color:red}em{color:blue}a{color:green}code{color:#8040c0}</style><p>same <em>same</em> <a href='/next'><strong>same</strong></a> <code>same</code></p>"#;
    let url=Url::parse("https://fixture.invalid/").unwrap();
    let document=parse_coloured_document(html,&url,&Limits::DEFAULT);
    let plain=parse_document(html,&url,&Limits::DEFAULT);
    assert_eq!(document.visible_text(),plain.visible_text());
    assert_eq!(document.links,plain.links);
    let Block::Paragraph{content,..}=&document.blocks[0] else {panic!()};
    let mut actual=Vec::new();colours(content,&mut actual);
    assert!(actual.iter().any(|(t,c,_)|t.contains("same")&&*c==[255,0,0]),"{actual:?}");
    assert!(actual.iter().any(|(t,c,_)|t=="same"&&*c==[0,0,255]),"{actual:?}");
    assert!(actual.iter().any(|(t,c,_)|t=="same"&&*c==[0,128,0]),"{actual:?}");
    assert!(actual.iter().any(|(t,c,code)|t=="same"&&*c==[128,64,192]&&*code),"{actual:?}");
}
#[test]
fn forms_anchors_and_unstyled_entry_are_preserved() {
    let html=br#"<!doctype html><p id='top' style='color:red'>same</p><form action='/search'><input name=q value=ink></form><p id='end'>Tail</p>"#;
    let url=Url::parse("https://fixture.invalid/").unwrap();
    let coloured=parse_coloured_document(html,&url,&Limits::DEFAULT);
    let plain=parse_document(html,&url,&Limits::DEFAULT);
    assert_eq!(coloured.visible_text(),plain.visible_text());
    assert!(coloured.blocks.iter().any(|b|matches!(b,Block::Form(_))));
    assert!(matches!(&coloured.blocks[0],Block::Paragraph{anchor:Some(a),..} if a=="top"));
    assert!(matches!(&plain.blocks[0],Block::Paragraph{content,..} if matches!(&content[0],Inline::Text(_))));
}
#[test]
fn adjacent_colours_collapse_shared_spaces_and_keep_background_inheritance() {
    let html=br#"<!doctype html><p style='background-color:#ddeeff'><span style='color:red'>  same </span><span style='color:blue'> same  </span></p>"#;
    let url=Url::parse("https://fixture.invalid/").unwrap();
    let doc=parse_coloured_document(html,&url,&Limits::DEFAULT);
    assert_eq!(doc.visible_text(),"same same\n");
    let Block::Paragraph{content,..}=&doc.blocks[0] else {panic!()};
    assert!(content.iter().all(|i|matches!(i,Inline::ColouredText{background:Some([221,238,255]),..})));
}

#[test]
fn raw_coloured_pre_does_not_reveal_hidden_source_text_or_trim_indents(){
    let html=br#"<!doctype html><style>.gone{display:none}</style><pre style='color:red'>
  kept<span hidden>private-hidden</span><span aria-hidden='true'>private-aria</span><span style='visibility:hidden'>private-inline</span><span class=gone>private-css</span>

    tail</pre>"#;
    let document=parse_coloured_document(html,&Url::parse("https://fixture.invalid/").unwrap(),&Limits::DEFAULT);
    assert_eq!(document.visible_text(),"  kept\n\n    tail\n");
    assert!(!document.visible_text().contains("private"));
}

#[test]
fn computed_treatments_do_not_duplicate_spaces_between_source_slices(){
    let html=br#"<p><span style='font-weight:700;color:red'>one </span><span style='font-style:italic;color:blue'> two </span><span style='color:green'> three</span></p>"#;
    let doc=parse_coloured_document(html,&Url::parse("https://fixture.invalid/").unwrap(),&Limits::DEFAULT);
    assert_eq!(doc.visible_text(),"one two three\n");
}
