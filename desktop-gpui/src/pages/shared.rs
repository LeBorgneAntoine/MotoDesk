//! Dossier partagé (port of `routes/SharedFolder.svelte` and `routes/shared/*`): the shop's
//! sensitive files (accounts, contracts, IDs), encrypted on disk. Administrators create
//! folders and choose who sees each one; anyone who can see a folder can add and open files
//! in it.
//!
//! A file is sent and fetched a piece at a time by the client; this screen only says which
//! file and where, and shows how far it is.

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use motodesk_client::ApiResult;
use motodesk_client::shared::{self, AuditEntry, SharedFile, SharedFolder, SharedListing, SharedStatus, Visibility};
use motodesk_ui::gpui::prelude::FluentBuilder as _;
use motodesk_ui::gpui::*;
use motodesk_ui::*;

use crate::backend::{ApiExt as _, PickExt as _, SaveExt as _};
use crate::format::{self, humanize_error};
use crate::router::Router;

/// How often the folder is read again while files are still arriving from another PC.
const SYNC_POLL: Duration = Duration::from_secs(5);
/// How often the lines of the files being sent are redrawn.
const UPLOAD_TICK: Duration = Duration::from_millis(150);
/// How long the line of a file that went in stays on screen.
const DONE_LINGER: Duration = Duration::from_secs(4);
const AUDIT_ENTRIES: u32 = 200;

// The states of a file being sent; it starts at zero, on its way.
const SENT: u8 = 1;
const FAILED: u8 = 2;

/// How far a file being sent is. Written by the task that sends it, read by the screen.
#[derive(Default)]
struct Progress {
    sent: AtomicU64,
    state: AtomicU8,
    error: Mutex<Option<String>>,
}

/// One line of the files being sent.
struct Upload {
    key: u64,
    name: String,
    size: u64,
    progress: Arc<Progress>,
}

/// A file waiting to be sent: where it is, and the folder it goes to.
struct Waiting {
    path: PathBuf,
    folder_id: String,
    progress: Arc<Progress>,
}

/// One line of a folder: a folder or a file.
#[derive(Clone)]
enum Entry {
    Folder(SharedFolder),
    File(SharedFile),
}

/// The form that creates a folder, or changes its name and who sees it.
struct FolderForm {
    editing: Option<SharedFolder>,
    name: TextModel,
    visibility: SelectModel,
    chosen: Visibility,
    busy: bool,
    error: Option<String>,
    _subscriptions: Vec<Subscription>,
}

struct RenameForm {
    file: SharedFile,
    name: TextModel,
    busy: bool,
    error: Option<String>,
    _subscription: Subscription,
}

#[derive(Default)]
struct Audit {
    entries: Vec<AuditEntry>,
    loading: bool,
    error: Option<String>,
}

/// What the confirmation of a deletion says.
fn delete_warning(entry: &Entry) -> (&'static str, String) {
    match entry {
        Entry::Folder(folder) => (
            "Supprimer ce dossier ?",
            format!(
                "« {} » et tout ce qu'il contient seront supprimés définitivement, sur tous les postes. Cette action est irréversible.",
                folder.name
            ),
        ),
        Entry::File(file) => {
            ("Supprimer ce fichier ?", format!("« {} » sera supprimé définitivement. Cette action est irréversible.", file.name))
        }
    }
}

/// What a batch of files that went in is announced with.
fn added_message(count: usize) -> String {
    if count == 1 { "Fichier ajouté.".into() } else { format!("{count} fichiers ajoutés.") }
}

pub struct SharedPage {
    status: Option<SharedStatus>,
    listing: Option<Rc<SharedListing>>,
    loading: bool,
    load_error: Option<String>,
    /// The folder shown; `None` for the top level. Kept in the route, so it is found again.
    folder_id: Option<String>,
    /// Counts the readings: an answer for an older one is dropped.
    loads: u64,

    uploads: Vec<Upload>,
    next_key: u64,
    waiting: Vec<Waiting>,
    sending: bool,

    folder_form: Option<FolderForm>,
    rename: Option<RenameForm>,
    audit: Option<Audit>,
}

impl SharedPage {
    pub fn new(_: &mut Window, cx: &mut Context<Self>) -> Self {
        let folder_id = Router::global(cx).read(cx).param("folder").map(str::to_string);
        // While files are still arriving from another PC, look again every few seconds: they
        // appear as soon as their last chunk is in.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(SYNC_POLL).await;
                let alive = this.update(cx, |this, cx| {
                    if this.status.as_ref().and_then(|status| status.usage).is_some_and(|usage| usage.syncing_files > 0) {
                        this.load(true, cx);
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
        let mut page = Self {
            status: None,
            listing: None,
            loading: true,
            load_error: None,
            folder_id,
            loads: 0,
            uploads: Vec::new(),
            next_key: 1,
            waiting: Vec::new(),
            sending: false,
            folder_form: None,
            rename: None,
            audit: None,
        };
        page.load(false, cx);
        page
    }

    fn can_manage(&self) -> bool {
        self.listing.as_ref().is_some_and(|listing| listing.can_manage)
    }

    fn current_folder(&self) -> Option<&SharedFolder> {
        self.listing.as_ref().and_then(|listing| listing.folder.as_ref())
    }

    // ---- reading -------------------------------------------------------------------------------

    /// Read the folder again. `quiet` keeps what is on screen until the answer is there.
    fn load(&mut self, quiet: bool, cx: &mut Context<Self>) {
        self.loads += 1;
        let load = self.loads;
        let folder = self.folder_id.clone();
        self.loading = !quiet;
        self.load_error = None;
        cx.notify();
        cx.api(
            move |client| async move {
                let status = shared::status(&client).await?;
                let listing = if status.available { Some(shared::browse(&client, folder.as_deref()).await?) } else { None };
                Ok((status, listing))
            },
            move |this, result: ApiResult<(SharedStatus, Option<SharedListing>)>, cx| {
                if this.loads != load {
                    return;
                }
                this.loading = false;
                match result {
                    Ok((status, listing)) => {
                        this.status = Some(status);
                        this.listing = listing.map(Rc::new);
                    }
                    // A folder that vanished (deleted, or made private) sends back to the top.
                    Err(error) if error.is_not_found() && this.folder_id.is_some() => {
                        this.open_folder(None, cx);
                        return;
                    }
                    Err(error) => this.load_error = Some(humanize_error(&error, "Dossier partagé indisponible.")),
                }
                cx.notify();
            },
        );
    }

    fn open_folder(&mut self, folder_id: Option<String>, cx: &mut Context<Self>) {
        self.folder_id = folder_id.clone();
        Router::global(cx).update(cx, |router, _| router.set_param("folder", folder_id));
        self.load(false, cx);
    }

    // ---- files on their way in -----------------------------------------------------------------

    fn pick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.pick_files(window, "Ajouter des fichiers", |this, picked, window, cx| match picked {
            Ok(paths) => this.enqueue(paths, window, cx),
            Err(error) => toast::error(format!("Impossible d'ouvrir la sélection : {error}"), window, cx),
        });
    }

    /// Queue files for the folder shown. Folders are not taken: only the files themselves.
    fn enqueue(&mut self, paths: Vec<PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        if paths.is_empty() {
            return;
        }
        let Some(folder_id) = self.current_folder().map(|folder| folder.id.clone()) else {
            toast::error("Ouvrez d'abord un dossier pour y ajouter des fichiers.", window, cx);
            return;
        };
        let (folders, files): (Vec<PathBuf>, Vec<PathBuf>) = paths.into_iter().partition(|path| path.is_dir());
        if !folders.is_empty() {
            toast::error("Les dossiers ne sont pas pris en charge : déposez les fichiers eux-mêmes.", window, cx);
        }
        for path in files {
            let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
            let size = std::fs::metadata(&path).map(|file| file.len()).unwrap_or_default();
            let progress = Arc::new(Progress::default());
            self.uploads.push(Upload { key: self.next_key, name, size, progress: progress.clone() });
            self.next_key += 1;
            self.waiting.push(Waiting { path, folder_id: folder_id.clone(), progress });
        }
        self.send_waiting(window, cx);
        cx.notify();
    }

    /// Send what waits, one file after the other. The files of a batch go on being sent even
    /// if the screen is left meanwhile.
    fn send_waiting(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.sending || self.waiting.is_empty() {
            return;
        }
        let batch = std::mem::take(&mut self.waiting);
        self.sending = true;
        // The lines move while files are being sent.
        cx.spawn(async move |this, cx| {
            while this.update(cx, |this, cx| (this.sending, cx.notify()).0).unwrap_or(false) {
                cx.background_executor().timer(UPLOAD_TICK).await;
            }
        })
        .detach();
        cx.api_in(
            window,
            move |client| async move {
                let mut added = 0usize;
                for file in batch {
                    let progress = file.progress.clone();
                    match shared::upload(&client, &file.folder_id, &file.path, |sent| progress.sent.store(sent, Ordering::Relaxed)).await {
                        Ok(_) => {
                            file.progress.state.store(SENT, Ordering::Relaxed);
                            added += 1;
                        }
                        Err(error) => {
                            *file.progress.error.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) =
                                Some(humanize_error(&error, "Envoi impossible."));
                            file.progress.state.store(FAILED, Ordering::Relaxed);
                        }
                    }
                }
                added
            },
            |this, added, window, cx| {
                this.sending = false;
                if added > 0 {
                    toast::success(added_message(added), window, cx);
                    this.load(true, cx);
                }
                // A line that is done goes away after a moment; a failure stays until dismissed.
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(DONE_LINGER).await;
                    let _ = this.update(cx, |this, cx| {
                        this.uploads.retain(|upload| upload.progress.state.load(Ordering::Relaxed) != SENT);
                        cx.notify();
                    });
                })
                .detach();
                this.send_waiting(window, cx);
                cx.notify();
            },
        );
    }

    // ---- a file on its way out -----------------------------------------------------------------

    fn download(&mut self, file: &SharedFile, window: &mut Window, cx: &mut Context<Self>) {
        let id = file.id.clone();
        cx.choose_save_path(window, &file.name, move |_, chosen, window, cx| match chosen {
            Ok(Some(path)) => cx.api_in(
                window,
                move |client| async move { shared::download_to(&client, &id, &path).await.map(|_| path) },
                |_, result, window, cx| match result {
                    Ok(path) => toast::success(format!("Fichier enregistré : {}", path.display()), window, cx),
                    Err(error) => toast::error(humanize_error(&error, "Ouverture impossible."), window, cx),
                },
            ),
            // The dialog was closed.
            Ok(None) => {}
            Err(error) => toast::error(format!("Enregistrement impossible : {error}"), window, cx),
        });
    }

    // ---- folders -------------------------------------------------------------------------------

    fn open_folder_form(&mut self, editing: Option<SharedFolder>, window: &mut Window, cx: &mut Context<Self>) {
        let chosen = editing.as_ref().map_or(Visibility::Admins, SharedFolder::visibility);
        let name = TextModel::builder()
            .placeholder("Comptabilité 2026")
            .value(editing.as_ref().map(|folder| folder.name.clone()).unwrap_or_default())
            .build(window, cx);
        let options = Visibility::ALL.into_iter().map(|visibility| SelectOption::new(visibility.id(), visibility.label())).collect();
        let visibility = SelectModel::new(options, Some(chosen.id()), window, cx);
        let subscriptions = vec![
            name.on_event(window, cx, |this: &mut Self, event, window, cx| {
                if matches!(event, TextEvent::Submitted { .. }) {
                    this.save_folder(window, cx);
                }
            }),
            visibility.on_change(window, cx, |this: &mut Self, picked, _, cx| {
                if let (Some(form), Some(picked)) = (&mut this.folder_form, picked) {
                    form.chosen = Visibility::from_id(&picked);
                    cx.notify();
                }
            }),
        ];
        let title = match (&editing, self.current_folder()) {
            (Some(_), _) => "Modifier le dossier".to_string(),
            (None, Some(parent)) => format!("Nouveau dossier dans « {} »", parent.name),
            (None, None) => "Nouveau dossier".to_string(),
        };
        let creating = editing.is_none();
        self.folder_form =
            Some(FolderForm { editing, name: name.clone(), visibility, chosen, busy: false, error: None, _subscriptions: subscriptions });

        let (for_body, for_footer) = (cx.entity(), cx.entity());
        Modal::new(title).width(px(460.)).open(
            window,
            cx,
            move |_, cx| {
                let m = cx.ui().m;
                let Some(form) = &for_body.read(cx).folder_form else { return element(div()) };
                element(
                    col()
                        .gap(m.space_4)
                        .children(form.error.clone().map(|error| Alert::danger().child(error)))
                        .child(FormField::new("Nom du dossier").required().child(Input::new(&form.name)))
                        .child(FormField::new("Qui peut le voir").hint(form.chosen.hint()).child(Select::new(&form.visibility))),
                )
            },
            move |_, cx| {
                let busy = for_footer.read(cx).folder_form.as_ref().is_some_and(|form| form.busy);
                let (cancel, save) = (for_footer.clone(), for_footer.clone());
                vec![
                    element(Button::new("shared-folder-cancel").label("Annuler").disabled(busy).on_click(move |_, window, cx| {
                        cancel.update(cx, |this, _| this.folder_form = None);
                        close_modal(window, cx);
                    })),
                    element(
                        Button::new("shared-folder-save")
                            .primary()
                            .label(if creating { "Créer le dossier" } else { "Enregistrer" })
                            .loading(busy)
                            .on_click(move |_, window, cx| save.update(cx, |this, cx| this.save_folder(window, cx))),
                    ),
                ]
            },
        );
        // The dialog takes the keyboard when it opens: the field is given it afterwards.
        name.focus_selected(window, cx);
    }

    fn save_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let parent = self.current_folder().map(|folder| folder.id.clone());
        let Some(form) = &mut self.folder_form else { return };
        if form.busy {
            return;
        }
        let Some(name) = form.name.trimmed(cx) else {
            form.error = Some("Le nom du dossier est obligatoire.".into());
            cx.notify();
            return;
        };
        let (editing, visibility) = (form.editing.as_ref().map(|folder| folder.id.clone()), form.chosen);
        form.busy = true;
        form.error = None;
        cx.notify();
        let creating = editing.is_none();
        cx.api_in(
            window,
            move |client| async move {
                match editing {
                    Some(id) => shared::update_folder(&client, &id, &name, visibility).await,
                    None => shared::create_folder(&client, parent.as_deref(), &name, visibility).await,
                }
            },
            move |this, result, window, cx| {
                match result {
                    Ok(_) => {
                        this.folder_form = None;
                        close_modal(window, cx);
                        toast::success(if creating { "Dossier créé." } else { "Dossier modifié." }, window, cx);
                        this.load(true, cx);
                    }
                    Err(error) => {
                        if let Some(form) = &mut this.folder_form {
                            form.busy = false;
                            form.error = Some(humanize_error(&error, "Enregistrement impossible."));
                        }
                    }
                }
                cx.notify();
            },
        );
    }

    // ---- renaming a file -----------------------------------------------------------------------

    fn open_rename(&mut self, file: SharedFile, window: &mut Window, cx: &mut Context<Self>) {
        let name = TextModel::builder().value(file.name.clone()).build(window, cx);
        let subscription = name.on_event(window, cx, |this: &mut Self, event, window, cx| {
            if matches!(event, TextEvent::Submitted { .. }) {
                this.save_rename(window, cx);
            }
        });
        self.rename = Some(RenameForm { file, name: name.clone(), busy: false, error: None, _subscription: subscription });

        let (for_body, for_footer) = (cx.entity(), cx.entity());
        Modal::new("Renommer le fichier").width(px(460.)).open(
            window,
            cx,
            move |_, cx| {
                let m = cx.ui().m;
                let Some(form) = &for_body.read(cx).rename else { return element(div()) };
                element(
                    col()
                        .gap(m.space_4)
                        .children(form.error.clone().map(|error| Alert::danger().child(error)))
                        .child(FormField::new("Nom du fichier").required().child(Input::new(&form.name))),
                )
            },
            move |_, cx| {
                let busy = for_footer.read(cx).rename.as_ref().is_some_and(|form| form.busy);
                let (cancel, save) = (for_footer.clone(), for_footer.clone());
                vec![
                    element(Button::new("shared-rename-cancel").label("Annuler").disabled(busy).on_click(move |_, window, cx| {
                        cancel.update(cx, |this, _| this.rename = None);
                        close_modal(window, cx);
                    })),
                    element(
                        Button::new("shared-rename-save")
                            .primary()
                            .label("Renommer")
                            .loading(busy)
                            .on_click(move |_, window, cx| save.update(cx, |this, cx| this.save_rename(window, cx))),
                    ),
                ]
            },
        );
        name.focus_selected(window, cx);
    }

    fn save_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = &mut self.rename else { return };
        if form.busy {
            return;
        }
        let Some(name) = form.name.trimmed(cx) else {
            form.error = Some("Le nom du fichier est obligatoire.".into());
            cx.notify();
            return;
        };
        let id = form.file.id.clone();
        form.busy = true;
        form.error = None;
        cx.notify();
        cx.api_in(
            window,
            move |client| async move { shared::rename_file(&client, &id, &name).await },
            |this, result, window, cx| {
                match result {
                    Ok(_) => {
                        this.rename = None;
                        close_modal(window, cx);
                        toast::success("Fichier renommé.", window, cx);
                        this.load(true, cx);
                    }
                    Err(error) => {
                        if let Some(form) = &mut this.rename {
                            form.busy = false;
                            form.error = Some(humanize_error(&error, "Renommage impossible."));
                        }
                    }
                }
                cx.notify();
            },
        );
    }

    // ---- deleting ------------------------------------------------------------------------------

    fn ask_delete(&mut self, entry: Entry, window: &mut Window, cx: &mut Context<Self>) {
        let (title, message) = delete_warning(&entry);
        let page = cx.entity();
        Confirm::new(title, message).confirm_label("Supprimer").danger().open(window, cx, move |window, cx| {
            let entry = entry.clone();
            page.update(cx, |this, cx| this.delete(entry, window, cx));
        });
    }

    fn delete(&mut self, entry: Entry, window: &mut Window, cx: &mut Context<Self>) {
        cx.api_in(
            window,
            move |client| async move {
                match entry {
                    Entry::Folder(folder) => shared::delete_folder(&client, &folder.id).await,
                    Entry::File(file) => shared::delete_file(&client, &file.id).await,
                }
            },
            |this, result, window, cx| match result {
                Ok(()) => {
                    toast::success("Supprimé.", window, cx);
                    this.load(true, cx);
                }
                Err(error) => toast::error(humanize_error(&error, "Suppression impossible."), window, cx),
            },
        );
    }

    // ---- who did what --------------------------------------------------------------------------

    fn open_audit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.audit = Some(Audit { loading: true, ..Default::default() });
        cx.api(
            |client| async move { shared::audit(&client, AUDIT_ENTRIES).await },
            |this, result, cx| {
                let Some(audit) = &mut this.audit else { return };
                audit.loading = false;
                match result {
                    Ok(entries) => audit.entries = entries,
                    Err(error) => audit.error = Some(humanize_error(&error, "Historique indisponible.")),
                }
                cx.notify();
            },
        );
        let for_body = cx.entity();
        Modal::new("Historique du dossier partagé").width(px(640.)).open(
            window,
            cx,
            move |_, cx| {
                let theme = cx.ui().clone();
                let (c, m) = (theme.colors, theme.m);
                let Some(audit) = &for_body.read(cx).audit else { return element(div()) };
                if let Some(error) = audit.error.clone() {
                    return element(Alert::danger().child(error));
                }
                if audit.loading {
                    return element(div().h(px(140.)).child(StateView::loading().compact().title("Chargement…")));
                }
                if audit.entries.is_empty() {
                    return element(
                        div().h(px(180.)).child(
                            StateView::empty()
                                .compact()
                                .icon(IconName::Clock)
                                .title("Rien pour le moment")
                                .body("Les ouvertures, ajouts et suppressions de fichiers apparaîtront ici."),
                        ),
                    );
                }
                let lines = audit.entries.iter().map(|entry| {
                    row()
                        .items_start()
                        .gap(m.space_3)
                        .py(m.space_1)
                        .fs(m.fs_sm)
                        .child(div().w(px(140.)).flex_none().text_color(c.text_muted).child(format::date_time(&entry.at)))
                        .child(
                            row()
                                .min_w_0()
                                .flex_wrap()
                                .gap(m.space_1)
                                .child(div().font_weight(FontWeight::SEMIBOLD).text_color(c.text_strong).child(entry.who().to_string()))
                                .child(shared::audit_label(&entry.action).to_string())
                                .children(entry.detail.clone().map(|detail| div().text_color(c.text_strong).child(detail))),
                        )
                });
                element(
                    col()
                        .gap(m.space_3)
                        .child(div().id("shared-audit").max_h(px(420.)).overflow_y_scroll().children(lines.collect::<Vec<_>>()))
                        .child(div().fs(m.fs_xs).text_color(c.text_muted).child("Cet historique est tenu sur ce poste uniquement.")),
                )
            },
            |_, _| vec![element(Button::new("shared-audit-close").label("Fermer").on_click(|_, window, cx| close_modal(window, cx)))],
        );
    }

    // ---- drawing -------------------------------------------------------------------------------

    fn toolbar(&self, cx: &mut Context<Self>) -> PageToolbar {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let status = self.status.as_ref();
        let available = status.is_some_and(|status| status.available);
        let in_folder = self.current_folder().is_some();

        let actions = row()
            .gap(m.space_2)
            .when(status.is_some_and(|status| status.is_admin), |this| {
                this.child(
                    IconButton::new("shared-audit", IconName::Clock)
                        .title("Historique des accès")
                        .size(IconButtonSize::Md)
                        .on_click(cx.listener(|this, _, window, cx| this.open_audit(window, cx))),
                )
            })
            .when(available && self.can_manage(), |this| {
                this.child(
                    Button::new("shared-new-folder")
                        .icon(IconName::FolderPlus)
                        .label("Nouveau dossier")
                        .on_click(cx.listener(|this, _, window, cx| this.open_folder_form(None, window, cx))),
                )
            })
            .when(available, |this| {
                this.child(
                    Button::new("shared-add-files")
                        .primary()
                        .icon(IconName::Upload)
                        .label("Ajouter des fichiers")
                        .disabled(!in_folder)
                        .on_click(cx.listener(|this, _, window, cx| this.pick(window, cx))),
                )
            });

        let toolbar = PageToolbar::new("Dossier partagé")
            .icon(IconName::Folder)
            .subtitle(shared::storage_sentence(status.is_some_and(|status| status.replicated)))
            .trailing(actions);
        let Some(listing) = &self.listing else { return toolbar };

        // The path from the top down to the folder shown.
        let last = listing.breadcrumb.len().saturating_sub(1);
        let mut crumbs = row().flex_wrap().gap(m.space_1).child(
            Button::new("shared-crumb-top")
                .link()
                .small()
                .icon(IconName::Folder)
                .label("Dossier partagé")
                .on_click(cx.listener(|this, _, _, cx| this.open_folder(None, cx))),
        );
        for (index, crumb) in listing.breadcrumb.iter().enumerate() {
            crumbs = crumbs.child(div().text_color(c.text_subtle).child(Icon::new(IconName::ChevronRight).sm()));
            crumbs = if index == last {
                crumbs.child(div().fs(m.fs_sm).font_weight(FontWeight::SEMIBOLD).text_color(c.text_strong).child(crumb.name.clone()))
            } else {
                let id = crumb.id.clone();
                crumbs.child(
                    Button::new(("shared-crumb", index))
                        .link()
                        .small()
                        .label(crumb.name.clone())
                        .on_click(cx.listener(move |this, _, _, cx| this.open_folder(Some(id.clone()), cx))),
                )
            };
        }
        toolbar.below(crumbs)
    }

    fn upload_lines(&self, cx: &mut Context<Self>) -> Option<Div> {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        if self.uploads.is_empty() {
            return None;
        }
        let lines = self.uploads.iter().map(|upload| {
            let state = upload.progress.state.load(Ordering::Relaxed);
            let sent = upload.progress.sent.load(Ordering::Relaxed);
            let line = row().gap(m.space_3).px(m.space_3).py(m.space_2).bg(c.bg_surface).border_1().border_color(c.border).fs(m.fs_sm);
            let name = div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().child(upload.name.clone());
            match state {
                FAILED => {
                    let key = upload.key;
                    let error = upload.progress.error.lock().ok().and_then(|error| error.clone()).unwrap_or_else(|| "Envoi impossible.".into());
                    line.child(div().text_color(c.danger).child(Icon::new(IconName::CircleAlert).sm()))
                        .child(name)
                        .child(div().flex_none().max_w(relative(0.5)).text_color(c.danger).child(error))
                        .child(IconButton::new(("shared-upload-dismiss", key as usize), IconName::X).title("Ignorer").on_click(cx.listener(
                            move |this, _, _, cx| {
                                this.uploads.retain(|upload| upload.key != key);
                                cx.notify();
                            },
                        )))
                }
                SENT => line
                    .child(div().text_color(c.success).child(Icon::new(IconName::CircleCheck).sm()))
                    .child(name)
                    .child(div().flex_none().text_color(c.text_muted).child("Terminé")),
                _ => {
                    let percent = shared::percent(sent, upload.size);
                    line.child(div().text_color(c.text_muted).child(Icon::new(IconName::Upload).sm()))
                        .child(name)
                        .child(div().w(px(180.)).flex_none().child(ProgressBar::new(percent)))
                        .child(div().w(px(48.)).flex_none().text_color(c.text_muted).child(format!("{percent} %")))
                }
            }
        });
        Some(col().flex_none().gap(m.space_1).children(lines))
    }

    fn table(&self, listing: &Rc<SharedListing>, cx: &mut Context<Self>) -> DataTable {
        let can_manage = listing.can_manage;
        let entries: Rc<Vec<Entry>> =
            Rc::new(listing.folders.iter().cloned().map(Entry::Folder).chain(listing.files.iter().cloned().map(Entry::File)).collect());
        let columns = vec![
            Column::new("Nom").flex(3.),
            Column::new("Visible par").fixed(px(200.)),
            Column::new("Taille").fixed(px(120.)).right(),
            Column::new("Ajouté le").fixed(px(170.)),
            Column::new("").fixed(px(if can_manage { 132. } else { 60. })).right(),
        ];
        let (cells, page) = (entries.clone(), cx.entity());
        let table = DataTable::new("shared-entries", columns, entries.len(), move |ix, column, _, cx| {
            let theme = cx.ui().clone();
            let (c, m) = (theme.colors, theme.m);
            let Some(entry) = cells.get(ix) else { return div().into_any_element() };
            match (column, entry) {
                (0, Entry::Folder(folder)) => row()
                    .gap(m.space_2)
                    .child(div().text_color(c.accent).child(Icon::new(IconName::Folder)))
                    .child(div().font_weight(FontWeight::MEDIUM).text_color(c.text_strong).child(folder.name.clone()))
                    .into_any_element(),
                (0, Entry::File(file)) => row()
                    .gap(m.space_2)
                    .child(div().text_color(c.text_muted).child(Icon::new(IconName::File)))
                    .child(file.name.clone())
                    .into_any_element(),
                (1, Entry::Folder(folder)) => match folder.visibility() {
                    Visibility::Admins => Tag::new(Visibility::Admins.label()).status(Status::Warning).icon(IconName::Lock).into_any_element(),
                    open => Tag::new(open.label()).status(Status::Info).icon(IconName::Users).into_any_element(),
                },
                (2, Entry::File(file)) => shared::file_size(file.size_bytes).into_any_element(),
                (3, Entry::Folder(folder)) => format::date_time(&folder.created_at).into_any_element(),
                (3, Entry::File(file)) => format::date_time(&file.created_at).into_any_element(),
                (4, entry) => {
                    // The row itself opens or downloads: a button answers for itself only.
                    let button = |id: &'static str, icon: IconName, title: &'static str, act: fn(&mut SharedPage, Entry, &mut Window, &mut Context<SharedPage>)| {
                        let (page, entry) = (page.clone(), entry.clone());
                        IconButton::new((id, ix), icon).title(title).on_click(move |_, window, cx| {
                            cx.stop_propagation();
                            let entry = entry.clone();
                            page.update(cx, |this, cx| act(this, entry, window, cx));
                        })
                    };
                    let mut buttons = row().gap(m.space_1);
                    if let Entry::File(_) = entry {
                        buttons = buttons.child(button("shared-download", IconName::Download, "Télécharger", |this, entry, window, cx| {
                            if let Entry::File(file) = entry {
                                this.download(&file, window, cx);
                            }
                        }));
                    }
                    if can_manage {
                        let edit_title = if matches!(entry, Entry::Folder(_)) { "Modifier le dossier" } else { "Renommer" };
                        buttons = buttons
                            .child(button("shared-edit", IconName::Pencil, edit_title, |this, entry, window, cx| match entry {
                                Entry::Folder(folder) => this.open_folder_form(Some(folder), window, cx),
                                Entry::File(file) => this.open_rename(file, window, cx),
                            }))
                            .child(
                                button("shared-delete", IconName::Trash, "Supprimer", |this, entry, window, cx| this.ask_delete(entry, window, cx))
                                    .danger(),
                            );
                    }
                    buttons.into_any_element()
                }
                _ => div().into_any_element(),
            }
        });

        let (clicked, menus, menu_page) = (entries.clone(), entries, cx.entity());
        table
            .on_row_click(cx.listener(move |this, ix: &usize, window, cx| match clicked.get(*ix) {
                Some(Entry::Folder(folder)) => this.open_folder(Some(folder.id.clone()), cx),
                Some(Entry::File(file)) => this.download(&file.clone(), window, cx),
                None => {}
            }))
            .row_menu(move |ix, _, _| {
                let Some(entry) = menus.get(ix).cloned() else { return Vec::new() };
                let item = |label: &'static str, icon: IconName, act: fn(&mut SharedPage, Entry, &mut Window, &mut Context<SharedPage>)| {
                    let (page, entry) = (menu_page.clone(), entry.clone());
                    MenuEntry::new(label, move |window, cx| {
                        let entry = entry.clone();
                        page.update(cx, |this, cx| act(this, entry, window, cx));
                    })
                    .icon(icon)
                };
                let mut items = match &entry {
                    Entry::File(_) => vec![item("Télécharger", IconName::Download, |this, entry, window, cx| {
                        if let Entry::File(file) = entry {
                            this.download(&file, window, cx);
                        }
                    })],
                    Entry::Folder(_) => vec![item("Ouvrir", IconName::FolderOpen, |this, entry, _, cx| {
                        if let Entry::Folder(folder) = entry {
                            this.open_folder(Some(folder.id), cx);
                        }
                    })],
                };
                if can_manage {
                    items.push(match &entry {
                        Entry::File(_) => item("Renommer", IconName::Pencil, |this, entry, window, cx| {
                            if let Entry::File(file) = entry {
                                this.open_rename(file, window, cx);
                            }
                        }),
                        Entry::Folder(_) => item("Modifier…", IconName::Pencil, |this, entry, window, cx| {
                            if let Entry::Folder(folder) = entry {
                                this.open_folder_form(Some(folder), window, cx);
                            }
                        }),
                    });
                    items.push(MenuEntry::separator());
                    items.push(item("Supprimer", IconName::Trash, |this, entry, window, cx| this.ask_delete(entry, window, cx)));
                }
                items
            })
    }

    fn body(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        if let Some(error) = self.load_error.clone() {
            return StateView::error()
                .title("Dossier partagé indisponible")
                .body(error)
                .action("Réessayer", cx.listener(|this, _, _, cx| this.load(false, cx)))
                .into_any_element();
        }
        let Some(status) = &self.status else {
            return StateView::loading().title("Chargement…").into_any_element();
        };
        if !status.available {
            return StateView::empty()
                .icon(IconName::Lock)
                .title("Bientôt disponible sur ce poste")
                .body("Le dossier partagé protège vos fichiers avec une clé propre à votre atelier. Elle existe dès que le réseau local est créé ou que le compte en ligne est connecté.")
                .into_any_element();
        }
        let Some(listing) = self.listing.clone() else {
            return StateView::loading().title("Chargement…").into_any_element();
        };
        if listing.folders.is_empty() && listing.files.is_empty() {
            return if listing.folder.is_some() {
                StateView::empty()
                    .icon(IconName::FolderOpen)
                    .title("Ce dossier est vide")
                    .body("Déposez des fichiers ici, ou utilisez « Ajouter des fichiers ».")
                    .action("Ajouter des fichiers", cx.listener(|this, _, window, cx| this.pick(window, cx)))
                    .into_any_element()
            } else if listing.can_manage {
                StateView::empty()
                    .icon(IconName::FolderPlus)
                    .title("Aucun dossier pour le moment")
                    .body("Créez un dossier, par exemple « Comptabilité » réservé aux administrateurs, puis ajoutez-y vos fichiers.")
                    .action("Nouveau dossier", cx.listener(|this, _, window, cx| this.open_folder_form(None, window, cx)))
                    .into_any_element()
            } else {
                StateView::empty()
                    .icon(IconName::Lock)
                    .title("Aucun dossier ne vous est ouvert")
                    .body("Un administrateur choisit les dossiers que les employés peuvent voir.")
                    .into_any_element()
            };
        }

        let usage = status.usage;
        col()
            .size_full()
            .gap(m.space_3)
            .child(div().flex_1().min_h_0().border_1().border_color(c.border).child(self.table(&listing, cx)))
            .children(usage.and_then(|usage| shared::syncing_sentence(usage.syncing_files)).map(|sentence| Alert::info().child(sentence)))
            .children(usage.map(|usage| div().flex_none().fs(m.fs_xs).text_color(c.text_muted).child(shared::usage_sentence(&usage))))
            .into_any_element()
    }
}

impl Render for SharedPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let in_folder = self.current_folder().is_some();
        col()
            .id("shared-page")
            .size_full()
            .bg(c.bg_app)
            // Files dropped anywhere on the screen go to the folder shown.
            .when(in_folder, |this| this.drag_over::<ExternalPaths>(move |style, _, _, _| style.bg(c.accent_subtle)))
            .on_drop(cx.listener(|this, dropped: &ExternalPaths, window, cx| this.enqueue(dropped.paths().to_vec(), window, cx)))
            .child(self.toolbar(cx))
            .child(
                col()
                    .flex_1()
                    .min_h_0()
                    .gap(m.space_3)
                    .p(m.space_4)
                    .children(self.upload_lines(cx))
                    .child(div().flex_1().min_h_0().child(self.body(cx))),
            )
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: it would bring in GPUI's own `test` attribute in place of Rust's.
    use motodesk_client::shared::{SharedFile, SharedFolder};

    use super::{Entry, added_message, delete_warning};

    #[test]
    fn a_deletion_says_what_goes_and_that_it_cannot_be_undone() {
        let folder = SharedFolder { id: "d".into(), parent_id: None, name: "Comptabilité".into(), visibility: "admins".into(), created_at: String::new() };
        let (title, message) = delete_warning(&Entry::Folder(folder));
        assert_eq!(title, "Supprimer ce dossier ?");
        assert!(message.starts_with("« Comptabilité » et tout ce qu'il contient") && message.contains("sur tous les postes"));

        let file = SharedFile {
            id: "f".into(),
            folder_id: "d".into(),
            name: "Bilan.pdf".into(),
            mime: None,
            size_bytes: 0,
            state: "ready".into(),
            created_at: String::new(),
        };
        let (title, message) = delete_warning(&Entry::File(file));
        assert_eq!(title, "Supprimer ce fichier ?");
        assert!(message.starts_with("« Bilan.pdf » sera supprimé") && message.ends_with("irréversible."));
    }

    #[test]
    fn files_added_are_counted() {
        assert_eq!(added_message(1), "Fichier ajouté.");
        assert_eq!(added_message(3), "3 fichiers ajoutés.");
    }
}
