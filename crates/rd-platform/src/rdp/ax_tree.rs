//! The search in another app's accessibility tree (Swift `AccessibilityAutofill`), over any tree so it is
//! tested without the Accessibility API: the first empty secure text field, and a submit button by title.

/// How deep the search goes (Swift: depth < 15).
pub const MAX_DEPTH: usize = 15;

/// Button titles that submit the credential prompt, lowercased (Swift `submitTitles`).
pub const SUBMIT_TITLES: [&str; 7] = [
    "continue",
    "connect",
    "ok",
    "sign in",
    "folytatás",
    "csatlakozás",
    "bejelentkezés",
];

/// The accessibility role of a button.
pub const BUTTON_ROLE: &str = "AXButton";
/// The subrole of a password field.
pub const SECURE_TEXT_FIELD_SUBROLE: &str = "AXSecureTextField";

/// An element of an accessibility tree.
pub trait AxNode: Sized {
    /// `AXRole`.
    fn role(&self) -> Option<String>;
    /// `AXSubrole`.
    fn subrole(&self) -> Option<String>;
    /// `AXTitle`.
    fn title(&self) -> Option<String>;
    /// `AXValue` as text.
    fn value(&self) -> Option<String>;
    /// `AXChildren`, then `AXSheets`.
    fn children(&self) -> Vec<Self>;
}

/// The first secure text field without a value, depth first, at most [`MAX_DEPTH`] levels down.
pub fn find_empty_secure_field<N: AxNode>(node: N, depth: usize) -> Option<N> {
    if depth >= MAX_DEPTH {
        return None;
    }
    if node.subrole().as_deref() == Some(SECURE_TEXT_FIELD_SUBROLE)
        && node.value().unwrap_or_default().is_empty()
    {
        return Some(node);
    }
    node.children()
        .into_iter()
        .find_map(|child| find_empty_secure_field(child, depth + 1))
}

/// Whether `title` is one of [`SUBMIT_TITLES`], ignoring case.
#[must_use]
pub fn is_submit_title(title: &str) -> bool {
    let lower = title.to_lowercase();
    SUBMIT_TITLES.contains(&lower.as_str())
}

/// The first button titled like a submit button, depth first, at most [`MAX_DEPTH`] levels down.
pub fn find_submit_button<N: AxNode>(node: N, depth: usize) -> Option<N> {
    if depth >= MAX_DEPTH {
        return None;
    }
    if node.role().as_deref() == Some(BUTTON_ROLE) && node.title().is_some_and(|t| is_submit_title(&t)) {
        return Some(node);
    }
    node.children()
        .into_iter()
        .find_map(|child| find_submit_button(child, depth + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Default, PartialEq)]
    struct Fake {
        name: &'static str,
        role: Option<&'static str>,
        subrole: Option<&'static str>,
        title: Option<&'static str>,
        value: Option<&'static str>,
        children: Vec<Fake>,
    }

    impl AxNode for &Fake {
        fn role(&self) -> Option<String> {
            self.role.map(str::to_owned)
        }
        fn subrole(&self) -> Option<String> {
            self.subrole.map(str::to_owned)
        }
        fn title(&self) -> Option<String> {
            self.title.map(str::to_owned)
        }
        fn value(&self) -> Option<String> {
            self.value.map(str::to_owned)
        }
        fn children(&self) -> Vec<Self> {
            self.children.iter().collect()
        }
    }

    fn node(name: &'static str, children: Vec<Fake>) -> Fake {
        Fake {
            name,
            children,
            ..Fake::default()
        }
    }

    fn secure(name: &'static str, value: Option<&'static str>) -> Fake {
        Fake {
            name,
            subrole: Some(SECURE_TEXT_FIELD_SUBROLE),
            value,
            ..Fake::default()
        }
    }

    fn button(name: &'static str, title: &'static str) -> Fake {
        Fake {
            name,
            role: Some(BUTTON_ROLE),
            title: Some(title),
            ..Fake::default()
        }
    }

    #[test]
    fn finds_the_first_empty_secure_field() {
        let window = node(
            "window",
            vec![
                node("group", vec![secure("filled", Some("x"))]),
                node(
                    "sheet",
                    vec![node(
                        "form",
                        vec![secure("empty", Some("")), secure("later", None)],
                    )],
                ),
            ],
        );
        assert_eq!(find_empty_secure_field(&window, 0).map(|n| n.name), Some("empty"));
        assert_eq!(
            find_empty_secure_field(&node("w", vec![secure("none", None)]), 0).map(|n| n.name),
            Some("none")
        );
        assert_eq!(
            find_empty_secure_field(&node("w", vec![secure("x", Some("pw"))]), 0),
            None
        );
    }

    #[test]
    fn stops_at_depth_fifteen() {
        let mut deep = secure("deep", None);
        for _ in 0..14 {
            deep = node("level", vec![deep]);
        }
        // The field is 14 levels below the root: depth 14 is still searched.
        assert_eq!(find_empty_secure_field(&deep, 0).map(|n| n.name), Some("deep"));
        let deeper = node("level", vec![deep]);
        assert_eq!(find_empty_secure_field(&deeper, 0), None);
    }

    #[test]
    fn finds_submit_buttons_by_title_in_any_case() {
        let window = node(
            "window",
            vec![
                button("cancel", "Cancel"),
                node("bar", vec![button("go", "Continue")]),
                button("hu", "Csatlakozás"),
            ],
        );
        assert_eq!(find_submit_button(&window, 0).map(|n| n.name), Some("go"));
        assert!(is_submit_title("SIGN IN"));
        assert!(is_submit_title("bejelentkezés"));
        assert!(!is_submit_title("Cancel"));
        let titled_text = Fake {
            name: "label",
            title: Some("OK"),
            ..Fake::default()
        };
        assert_eq!(find_submit_button(&node("w", vec![titled_text]), 0), None);
    }
}
