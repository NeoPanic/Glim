//! Decode literal XML text and predefined/numeric references without DTD access.
use anyhow::Result;
use quick_xml::events::Event;
pub(crate) fn event_text(event: &Event<'_>) -> Result<Option<String>> {
    Ok(match event {
        Event::Text(text) => Some(quick_xml::escape::unescape(&text.decode()?)?.into_owned()),
        Event::CData(text) => Some(text.decode()?.into_owned()),
        Event::GeneralRef(reference) => Some(if let Some(ch) = reference.resolve_char_ref()? {
            ch.to_string()
        } else {
            match reference.decode()?.as_ref() {
                "amp" => "&",
                "lt" => "<",
                "gt" => ">",
                "apos" => "'",
                "quot" => "\"",
                _ => anyhow::bail!("Unresolved XML character reference"),
            }
            .to_owned()
        }),
        _ => None,
    })
}
