use super::media::{GifReader, InlineImageReader};
use glim_services::markup_images::{self, PictureSources};
use gpui_kit::component::ActiveTheme;
use gpui_kit::*;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
pub(super) struct Pictures {
    pub sources: Arc<HashMap<String, PictureSources>>,
    pub animations: Arc<HashMap<String, Entity<GifReader>>>,
    pub stills: Arc<HashMap<String, Entity<InlineImageReader>>>,
    references: HashSet<String>,
    animation_subscriptions: Vec<Subscription>,
    still_subscriptions: Vec<Subscription>,
    base: url::Url,
    active: bool,
    task: Option<Task<()>>,
}
impl Pictures {
    pub fn new(text: String, html: bool, base: url::Url, cx: &mut Context<Self>) -> Self {
        let mut pictures = Self {
            sources: Arc::new(HashMap::new()),
            animations: Arc::new(HashMap::new()),
            animation_subscriptions: Vec::new(),
            stills: Arc::new(HashMap::new()),
            references: HashSet::new(),
            still_subscriptions: Vec::new(),
            base,
            active: true,
            task: None,
        };
        pictures.load(text, html, cx);
        pictures
    }
    pub fn load(&mut self, text: String, html: bool, cx: &mut Context<Self>) {
        self.sources = Arc::new(HashMap::new());
        let work = cx.background_executor().spawn(async move {
            let sources = markup_images::picture_sources(&text, html);
            let references = markup_images::image_uris(&text, html);
            let mut images = references.clone();
            for variants in sources.values() {
                images.extend(variants.light.iter().chain(variants.dark.iter()).cloned());
            }
            (sources, images, references)
        });
        self.task = Some(cx.spawn(async move |view, cx| {
            let (sources, images, references) = work.await;
            let _ = view.update(cx, |view, cx| {
                let mut animations = HashMap::new();
                let mut stills = HashMap::new();
                view.still_subscriptions.clear();
                view.animation_subscriptions.clear();
                for uri in images {
                    let Ok(url) = view.base.join(&uri) else {
                        continue;
                    };
                    if !matches!(url.scheme(), "file" | "http" | "https") {
                        continue;
                    }
                    let key = url.to_string();
                    if !url.path().to_ascii_lowercase().ends_with(".gif") {
                        if stills.contains_key(&key) {
                            continue;
                        }
                        let image = view
                            .stills
                            .get(&key)
                            .cloned()
                            .unwrap_or_else(|| cx.new(|cx| InlineImageReader::new(url, cx)));
                        view.still_subscriptions
                            .push(cx.observe(&image, |_, _, cx| cx.notify()));
                        stills.insert(key, image);
                        continue;
                    }
                    if animations.contains_key(&key) {
                        continue;
                    }
                    let animation = view
                        .animations
                        .get(&key)
                        .cloned()
                        .unwrap_or_else(|| cx.new(|cx| GifReader::from_url(url, cx)));
                    animation.update(cx, |animation, cx| animation.set_active(view.active, cx));
                    view.animation_subscriptions
                        .push(cx.observe(&animation, |_, _, cx| cx.notify()));
                    animations.insert(key, animation);
                }
                view.animations = Arc::new(animations);
                view.sources = Arc::new(sources);
                view.stills = Arc::new(stills);
                view.references = references;
                view.set_active(view.active, cx);
                view.task = None;
                cx.notify();
            });
        }));
        cx.notify();
    }
    pub fn refresh_local_images(&mut self, cx: &mut Context<Self>) {
        if !self.active {
            return;
        }
        for image in self.stills.values() {
            image.update(cx, |image, cx| image.refresh(cx));
        }
    }
    pub fn set_active(&mut self, active: bool, cx: &mut App) {
        self.active = active;
        let dark = cx.theme().is_dark();
        let selected = self
            .references
            .iter()
            .filter_map(|uri| {
                let uri = self
                    .sources
                    .get(uri)
                    .and_then(|sources| {
                        if dark {
                            sources.dark.as_deref()
                        } else {
                            sources.light.as_deref()
                        }
                    })
                    .unwrap_or(uri);
                self.base.join(uri).ok().map(|url| url.to_string())
            })
            .collect::<HashSet<_>>();
        for (uri, animation) in self.animations.iter() {
            animation.update(cx, |animation, cx| {
                animation.set_active(active && selected.contains(uri), cx)
            });
        }
        for (uri, image) in self.stills.iter() {
            image.update(cx, |image, cx| {
                image.set_active(active && selected.contains(uri), cx)
            });
        }
    }
}
