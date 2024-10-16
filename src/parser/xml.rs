use anpa::combinators::{attempt, left, many, many_to_vec, middle, no_separator, right, succeed};
use anpa::core::{ParserExt, StrParser};
use anpa::{create_parser, defer_parser, item, left, or, right, tuplify, variadic};
use anpa::parsers::{item_if, item_while, seq, until_seq};
use crate::parser::core::eat;

#[derive(Debug, PartialEq)]
pub(super) struct XmlTag<'a> {
    name: &'a str,
    attributes: Vec<Attribute<'a>>,
    content: Option<NodeContent<'a>>,
}

#[derive(Debug, PartialEq)]
struct Attribute<'a> {
    name: &'a str,
    value: &'a str,
}

#[derive(Debug, PartialEq)]
enum NodeContent<'a> {
    Tags(Vec<XmlTag<'a>>),
    Text(&'a str),
}

impl XmlTag<'_> {
    fn with_children<'a>(
        name: &'a str,
        attributes: Vec<Attribute<'a>>,
        children: Vec<XmlTag<'a>>,
    ) -> XmlTag<'a> {
        XmlTag { name, attributes, content: Some(NodeContent::Tags(children)) }
    }

    fn with_text<'a>(
        name: &'a str,
        attributes: Vec<Attribute<'a>>,
        text: &'a str,
    ) -> XmlTag<'a> {
        XmlTag { name, attributes, content: Some(NodeContent::Text(text)) }
    }

    fn new<'a>(
        name: &'a str,
        attributes: Vec<Attribute<'a>>,
    ) -> XmlTag<'a> {
        XmlTag { name, attributes, content: None }
    }
}

impl XmlTag<'_> {
    pub(super) fn children(&self) -> &[XmlTag<'_>] {
        static EMPTY: [XmlTag; 0] = [];
        match &self.content {
            Some(NodeContent::Tags(children)) => &children,
            _ => &EMPTY,
        }
    }

    pub(super) fn text(&self) -> Option<&str> {
        match &self.content {
            Some(NodeContent::Text(text)) => Some(text),
            _ => None,
        }
    }

    pub(super) fn has_attribute(&self, name: &str) -> &str {
        self.attributes.iter().find(|a| a.name == name).map(|a| a.value).unwrap_or("")
    }

    pub(super) fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes.iter().find(|a| a.name == name).map(|a| a.value)
    }
}

impl<'a> Attribute<'a> {
    fn new(name: &'a str, value: &'a str) -> Self {
        Self { name, value }
    }
}

fn comment<'a>() -> impl StrParser<'a, &'a str> {
    eat(right(seq("<!--"), until_seq("-->")))
}

fn attribute_value<'a>() -> impl StrParser<'a, &'a str> {
    right(item!('"'), until_seq("\""))
}

fn name_parser<'a>() -> impl StrParser<'a, &'a str> {
    item_while(|c: char| c.is_alphanumeric() || c == '-' || c == '_')
}

fn cdata<'a>() -> impl StrParser<'a, &'a str> {
    let valid_char = item_if(|c: char| c != ']');
    middle(seq("<![CDATA["), many(valid_char, true, no_separator()), seq("]]>"))
        .map(|s: &str| s.trim())
}

fn attribute<'a>() -> impl StrParser<'a, Attribute<'a>> {
    tuplify!(
        left(eat(name_parser()), eat(until_seq("="))),
        eat(attribute_value()),
    ).map(|(key, value)| Attribute::new(key.trim(), value))
}

fn xml_tag_with_children<'a>() -> impl StrParser<'a, XmlTag<'a>> {
    let attributes = attempt(many_to_vec(attribute(), true, no_separator()));
    let child_tags = attempt(many_to_vec(xml_parser(), true, no_separator()));

    let closing_tag = right!(succeed(item!('<')), item!('/'), name_parser(), eat(seq(">")));
    let comments = many_to_vec(comment(), true, no_separator());

    tuplify!(
        right!(comments, eat(item!('<')), name_parser()), // (discard comments) <tag
        left!(attributes, eat(seq(">")), comments),       // key="value"...> (discard comments)
        left!(child_tags, closing_tag),                   // recurse children
    ).map(|(name, attributes, children)| {
        if children.is_empty() {
            XmlTag::new(name, attributes)
        } else {
            XmlTag::with_children(name, attributes, children)
        }
    })
}

fn xml_tag_with_text<'a>() -> impl StrParser<'a, XmlTag<'a>> {
    let attributes = attempt(many_to_vec(attribute(), true, no_separator()));
    let closing_tag = right!(seq("</"), name_parser(), eat(item!('>')));
    let text = eat(item_while(|c: char| c != '<'));

    let dummy_text = eat(item_while(|c: char| c == ' '));
    let comments = many_to_vec(comment(), true, no_separator());

    tuplify!(
        right!(comments, item!('<'), name_parser()),
        attributes,
        or!(
            seq("/>"),
            middle(eat(item!('>')), text, closing_tag),
        )
    ).map(|(name, attributes, text)| {
        if text == "/>" {
            XmlTag::new(name, attributes)
        } else {
            XmlTag::with_text(name, attributes, text.trim())
        }
    })
}

pub(super) fn xml_parser<'a>() -> impl StrParser<'a, XmlTag<'a>> {
    defer_parser! {
        eat(or!(xml_tag_with_text(), xml_tag_with_children()))
    }
}


mod tests {
    use super::*;
    use anpa::core::parse;

    #[test]
    fn test_cdata() {
        [
            "<![CDATA[This is a CDATA]]>",
            r#"<![CDATA[

                This is a CDATA

            ]]>"#,
        ].iter().for_each(|input| {
            let p = cdata();
            let result = parse(p, input);

            assert_eq!(result.state, "");
            assert_eq!(result.result, Some("This is a CDATA"));
        });
    }

    #[test]
    fn parse_name() {
        let p = name_parser();
        let result = parse(p, "hello></hello>");

        assert_eq!(result.state, "></hello>");
        assert_eq!(result.result, Some("hello"));
    }

    #[test]
    fn parse_attribute_value() {
        let p = attribute_value();
        let result = parse(p, r#""This is a value" "#);

        assert_eq!(result.state, " ");
        assert_eq!(result.result, Some("This is a value"));
    }

    #[test]
    fn parse_attribute() {
        [
            r#"name="value" "#,
            r#"name =  "value" "#,
        ].iter().for_each(|s|{
            let p = attribute();
            let result = parse(p, s);
            assert_eq!(result.state, " ");
            assert_eq!(result.result, Some(Attribute::new("name", "value")));
        });

        let result = parse(attribute(), r#" name =  "value"></hello>"#);
        assert_eq!(result.state, "></hello>");
        assert_eq!(result.result, Some(Attribute::new("name", "value")));
    }

    #[test]
    fn parse_self_contained_xml_tag() {
        let p = xml_parser();
        let result = parse(p, "<tag key=\"value\"/>");

        assert_eq!(result.state, "");
        assert_eq!(result.result, Some(XmlTag::new("tag", vec![Attribute::new("key", "value")])));

        let input = r#"<keymap version="1" name="Eclipse copy" parent="Eclipse"/>"#;
        let result = parse(p, input);

        assert_eq!(result.state, "");
        assert_eq!(result.result, Some(XmlTag::new("keymap", vec![
            Attribute::new("version", "1"),
            Attribute::new("name", "Eclipse copy"),
            Attribute::new("parent", "Eclipse")]))
        );
    }

    #[test]
    fn parse_tag_with_text() {
        let p = xml_parser();
        let result = parse(p, "<tag key=\"value\">This is a text</tag>");

        assert_eq!(result.state, "");
        assert_eq!(result.result, Some(XmlTag::with_text("tag", vec![Attribute::new("key", "value")], "This is a text")));

        let result = parse(p, r#"<tag>
            text
        </tag>"#);

        assert_eq!(result.state, "");
        assert_eq!(result.result, Some(XmlTag::with_text("tag", Vec::new(), "text")));
    }

    #[test]
    fn test_jetbrains_xml_parser() {
        let p = xml_parser();

        let result = parse(p, r#"<keymap ></keymap>"#);
        assert_eq!(result.state, "");
        assert_eq!(result.result, Some(XmlTag::new("keymap", vec![])));

        let result = parse(p, r#"<keymap version="1" name="Eclipse copy" parent="Eclipse"></keymap>"#);
        assert_eq!(result.state, "");
        assert_eq!(result.result, Some(XmlTag::new("keymap", vec![Attribute::new("version", "1"), Attribute::new("name", "Eclipse copy"), Attribute::new("parent", "Eclipse")])));


        let result = parse(p, r#"<keymap version="1" name="Eclipse copy" parent="Eclipse">
    <action id="$Copy">
        <keyboard-shortcut first-keystroke="ctrl c" />
    </action>
    <action id="$Redo">
        <keyboard-shortcut first-keystroke="shift ctrl z" />
    </action>
    <action id=":cursive.repl.actions/jump-to-repl">
        <keyboard-shortcut first-keystroke="ctrl 2" />
    </action>
    <action id=":cursive.repl.actions/run-last-sexp">
        <keyboard-shortcut first-keystroke="ctrl 3" />
    </action>
    <action id=":cursive.repl.actions/sync-files">
        <keyboard-shortcut first-keystroke="shift ctrl r" />
    </action>
    <action id="ActivateMavenProjectsToolWindow">
        <keyboard-shortcut first-keystroke="f2" />
    </action>
    <action id="Build">
        <keyboard-shortcut first-keystroke="ctrl f9" />
    </action>
    <action id="BuildProject">
        <keyboard-shortcut first-keystroke="ctrl b" />
    </action>
    <action id="ChooseDebugConfiguration">
        <keyboard-shortcut first-keystroke="alt d" />
    </action>
    <action id="ChooseRunConfiguration">
        <keyboard-shortcut first-keystroke="alt r" />
    </action>
    <action id="CloseActiveTab" />
    <action id="CloseContent">
        <keyboard-shortcut first-keystroke="ctrl w" />
    </action>
    <action id="CollapseAll">
        <keyboard-shortcut first-keystroke="ctrl subtract" />
    </action>
    <action id="CollapseAllRegions">
        <keyboard-shortcut first-keystroke="shift ctrl divide" />
        <keyboard-shortcut first-keystroke="ctrl minus" />
    </action>
</keymap>"#);

        assert_eq!(result.state, "");
        assert_eq!(result.result, Some(XmlTag::with_children("tag", vec![Attribute::new("key", "value")], vec![XmlTag::new("tag2", vec![Attribute::new("key2", "value2")])])));
    }
}