use crate::tui::action::Action;
use cc_switch_lib::{AppState, AppType, Database, Provider, ProxyService, ProviderService};
use indexmap::IndexMap;
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Clone, Copy, PartialEq)]
pub enum Focus {
    AppTabs,
    MainPanel,
    Detail,
}

pub enum Modal {
    None,
    Help,
    Error(String),
}

pub struct App {
    pub running: bool,
    pub focus: Focus,
    pub modal: Modal,
    pub apps: Vec<AppType>,
    pub selected_app: usize,
    pub providers: IndexMap<String, Provider>,
    pub current_id: String,
    pub selected_provider: usize,
    pub detail_scroll: u16,
    pub tx: mpsc::UnboundedSender<Action>,
    pub db: Arc<Database>,
    pub proxy: ProxyService,
    pub version: String,
}

impl App {
    pub fn new(state: AppState, tx: mpsc::UnboundedSender<Action>) -> Self {
        let apps: Vec<AppType> = AppType::all().collect();
        let db = state.db.clone();
        let proxy = state.proxy_service.clone();
        Self {
            running: true,
            focus: Focus::MainPanel,
            modal: Modal::None,
            apps,
            selected_app: 0,
            providers: IndexMap::new(),
            current_id: String::new(),
            selected_provider: 0,
            detail_scroll: 0,
            tx, db, proxy,
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    pub fn load_current_app(&self) {
        let app_type = self.apps[self.selected_app].clone();
        let db = self.db.clone();
        let proxy = self.proxy.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let state = AppState { db, proxy_service: proxy };
            let providers = ProviderService::list(&state, app_type.clone()).unwrap_or_default();
            let current = ProviderService::current(&state, app_type.clone()).unwrap_or_default();
            let _ = tx.send(Action::ProvidersLoaded(app_type, providers, current));
        });
    }

    pub fn selected_provider_entry(&self) -> Option<(&String, &Provider)> {
        self.providers.get_index(self.selected_provider)
    }

    pub fn dispatch(&mut self, action: Action) {
        match action {
            Action::Quit => self.running = false,
            Action::FocusLeft => {
                self.focus = match self.focus {
                    Focus::Detail => Focus::MainPanel,
                    Focus::MainPanel => Focus::AppTabs,
                    Focus::AppTabs => Focus::AppTabs,
                };
            }
            Action::FocusRight => {
                self.focus = match self.focus {
                    Focus::AppTabs => Focus::MainPanel,
                    Focus::MainPanel => Focus::Detail,
                    Focus::Detail => Focus::Detail,
                };
            }
            Action::Up => self.navigate(-1),
            Action::Down => self.navigate(1),
            Action::Select => {
                if self.focus == Focus::AppTabs {
                    self.focus = Focus::MainPanel;
                } else {
                    self.do_switch_provider();
                }
            }
            Action::Back => {
                if !matches!(self.modal, Modal::None) {
                    self.modal = Modal::None;
                } else if self.focus != Focus::AppTabs {
                    self.focus = Focus::AppTabs;
                }
            }
            Action::ShowHelp => self.modal = Modal::Help,
            Action::ProvidersLoaded(_app, providers, current) => {
                let max = providers.len().saturating_sub(1);
                self.selected_provider = self.selected_provider.min(max);
                self.providers = providers;
                self.current_id = current;
                self.detail_scroll = 0;
            }
            Action::Error(msg) => self.modal = Modal::Error(msg),
        }
    }

    fn navigate(&mut self, delta: i32) {
        match self.focus {
            Focus::AppTabs => {
                let len = self.apps.len();
                if len == 0 { return; }
                self.selected_app = (self.selected_app as i32 + delta).rem_euclid(len as i32) as usize;
                self.load_current_app();
            }
            Focus::MainPanel => {
                let len = self.providers.len();
                if len == 0 { return; }
                self.selected_provider =
                    (self.selected_provider as i32 + delta).rem_euclid(len as i32) as usize;
                self.detail_scroll = 0;
            }
            Focus::Detail => {
                if delta > 0 {
                    self.detail_scroll = self.detail_scroll.saturating_add(1);
                } else {
                    self.detail_scroll = self.detail_scroll.saturating_sub(1);
                }
            }
        }
    }

    fn do_switch_provider(&self) {
        if let Some((id, _)) = self.selected_provider_entry() {
            let id = id.clone();
            let app_type = self.apps[self.selected_app].clone();
            let db = self.db.clone();
            let proxy = self.proxy.clone();
            let tx = self.tx.clone();
            tokio::spawn(async move {
                let state = AppState { db, proxy_service: proxy };
                match ProviderService::switch(&state, app_type.clone(), &id) {
                    Ok(()) => {
                        let providers =
                            ProviderService::list(&state, app_type.clone()).unwrap_or_default();
                        let current =
                            ProviderService::current(&state, app_type.clone()).unwrap_or_default();
                        let _ = tx.send(Action::ProvidersLoaded(app_type, providers, current));
                    }
                    Err(e) => {
                        let _ = tx.send(Action::Error(format!("切换失败: {e}")));
                    }
                }
            });
        }
    }
}
