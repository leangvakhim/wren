mod accounts;
mod attachments;
mod events;
mod history;
mod limits;
mod link;
mod login;
mod models;
mod providers;
mod reports;
mod sessions;

use std::collections::HashMap;

use tauri::ipc::Channel;
use tauri::{Manager as _, RunEvent, State};

use accounts::{Account, AccountStatus, AccountStore, AccountsView};
use events::{PlanLimits, UiEvent};
use history::{ConversationMeta, History};
use login::{LoginEvent, LoginManager};
use models::ModelInfo;
use link::Decision;
use providers::{Mode, Permission, Provider};
use sessions::{Manager, PaneConfig};

fn account_env(store: &AccountStore, cfg: &PaneConfig) -> Result<Vec<(String, String)>, String> {
    let no_account = || format!("No {} account in Wren yet. Open Accounts to add one.", cfg.provider.label());
    let id = cfg.account_id.clone().filter(|s| !s.is_empty()).ok_or_else(no_account)?;
    let acc = store.get(&id).map_err(|_| no_account())?;
    if acc.provider != cfg.provider {
        return Err("That account belongs to a different AI.".into());
    }
    store.env_for(&id)
}

// ---------------- conversations ----------------

#[tauri::command]
fn open_pane(
    state: State<'_, Manager>,
    store: State<'_, AccountStore>,
    pane_id: String,
    config: PaneConfig,
    on_event: Channel<UiEvent>,
) -> Result<(), String> {
    let env = account_env(&store, &config);
    state.open(pane_id, config, env, on_event)
}

#[tauri::command]
fn update_pane(
    state: State<'_, Manager>,
    store: State<'_, AccountStore>,
    pane_id: String,
    config: PaneConfig,
) -> Result<(), String> {
    let env = account_env(&store, &config);
    state.update(&pane_id, config, env)
}

/// Change model/effort; the conversation continues.
#[tauri::command]
fn set_model(state: State<'_, Manager>, pane_id: String, model: Option<String>, effort: Option<String>) -> Result<(), String> {
    state.set_model(&pane_id, model, effort)
}

/// Turn Study / Deep research on or off; the conversation continues.
#[tauri::command]
fn set_mode(state: State<'_, Manager>, pane_id: String, mode: Option<Mode>) -> Result<(), String> {
    state.set_mode(&pane_id, mode)
}

/// The user answered an approval card (Allow / Allow for this chat / Deny).
#[tauri::command]
fn answer_approval(state: State<'_, Manager>, pane_id: String, id: String, decision: Decision) -> Result<(), String> {
    state.answer(&pane_id, &id, decision)
}

/// Change Manual / Auto / Plan / Full access; the conversation continues.
#[tauri::command]
fn set_permission(state: State<'_, Manager>, pane_id: String, permission: Permission) -> Result<(), String> {
    state.set_permission(&pane_id, permission)
}

/// Deep research: save the final report into the project folder.
#[tauri::command]
fn save_report(cwd: String, stamp: String, text: String) -> Result<String, String> {
    reports::save(&cwd, &stamp, &text).map(|p| p.to_string_lossy().to_string())
}

/// The plan limits (5-hour / weekly, daily tokens) of one account, asked
/// from its own CLI. The CLI uses its own sign-in; Wren never sees tokens.
#[tauri::command]
async fn plan_limits(store: State<'_, AccountStore>, account_id: String) -> Result<PlanLimits, String> {
    let acc = store.get(&account_id)?;
    if acc.provider == Provider::Antigravity {
        return limits::read(acc.provider, &[]).await;
    }
    let env = store.env_for(&account_id)?;
    limits::read(acc.provider, &env).await
}

/// Open a saved conversation in a pane and continue it with the CLI's session id.
#[tauri::command]
fn resume_conversation(
    state: State<'_, Manager>,
    store: State<'_, AccountStore>,
    pane_id: String,
    config: PaneConfig,
    session_id: Option<String>,
) -> Result<(), String> {
    let env = account_env(&store, &config);
    state.resume(&pane_id, config, env, session_id)
}

/// Models and effort tiers this AI offers (asked from the CLI where possible).
#[tauri::command]
async fn list_models(store: State<'_, AccountStore>, provider: Provider, account_id: Option<String>) -> Result<Vec<ModelInfo>, String> {
    let env = account_id
        .and_then(|id| store.env_for(&id).ok())
        .unwrap_or_default();
    Ok(models::list_models(provider, &env).await)
}

// ---------------- conversation history (local only) ----------------

#[tauri::command]
fn list_conversations(history: State<'_, History>) -> Vec<ConversationMeta> {
    history.list()
}

#[tauri::command]
fn get_conversation(history: State<'_, History>, id: String) -> Result<serde_json::Value, String> {
    history.get(&id)
}

#[tauri::command]
fn save_conversation(history: State<'_, History>, conversation: serde_json::Value) -> Result<ConversationMeta, String> {
    history.save(conversation)
}

#[tauri::command]
fn delete_conversation(history: State<'_, History>, id: String) -> Result<(), String> {
    history.delete(&id)
}

#[tauri::command]
fn usage_records(history: State<'_, History>) -> Vec<serde_json::Value> {
    history.usage_records()
}

#[tauri::command]
fn rename_conversation(history: State<'_, History>, id: String, title: String) -> Result<(), String> {
    history.rename(&id, &title)
}

/// Send a message, with files attached in Wren (paths of private copies).
#[tauri::command]
async fn send_message(
    state: State<'_, Manager>,
    store: State<'_, attachments::Store>,
    pane_id: String,
    text: String,
    files: Option<Vec<String>>,
) -> Result<(), String> {
    let paths = files.unwrap_or_default();
    if paths.len() > attachments::MAX_FILES {
        return Err(format!("At most {} files per message.", attachments::MAX_FILES));
    }
    let out = paths.iter().map(|p| store.outgoing(p)).collect::<Result<Vec<_>, _>>()?;
    state.send(&pane_id, text, out)
}

/// Attach a pasted or dropped file: the raw bytes, with its name in the
/// `x-file-name` header (URL-encoded).
#[tauri::command]
fn attach_bytes(store: State<'_, attachments::Store>, request: tauri::ipc::Request<'_>) -> Result<attachments::Attachment, String> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err("No file data received.".into());
    };
    let name = request
        .headers()
        .get("x-file-name")
        .and_then(|v| v.to_str().ok())
        .map(percent_decode)
        .unwrap_or_else(|| "file".into());
    store.save(&name, bytes)
}

/// "%20" -> " " (names come URL-encoded so any language works in a header).
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// Attach a file picked in the file dialog (copied into Wren's folder).
#[tauri::command]
fn attach_file(store: State<'_, attachments::Store>, path: String) -> Result<attachments::Attachment, String> {
    store.import(&path)
}

/// Thumbnail of an attachment in a saved conversation.
#[tauri::command]
fn attachment_preview(store: State<'_, attachments::Store>, path: String) -> Result<String, String> {
    store.preview(&path)
}

#[tauri::command]
fn new_conversation(state: State<'_, Manager>, pane_id: String) -> Result<(), String> {
    state.new_conversation(&pane_id)
}

#[tauri::command]
fn stop_pane(state: State<'_, Manager>, pane_id: String) -> Result<(), String> {
    state.stop(&pane_id)
}

#[tauri::command]
fn close_pane(state: State<'_, Manager>, pane_id: String) -> Result<(), String> {
    state.close(&pane_id)
}

#[tauri::command]
async fn detect_clis() -> HashMap<Provider, Option<String>> {
    tauri::async_runtime::spawn_blocking(sessions::detect_clis)
        .await
        .unwrap_or_default()
}

/// New panes start here: ~/Desktop on macOS, the home folder elsewhere.
#[tauri::command]
fn default_folder(app: tauri::AppHandle) -> String {
    let dir = if cfg!(target_os = "macos") {
        app.path().desktop_dir().ok().filter(|d| d.is_dir())
    } else {
        None
    };
    dir.or_else(|| app.path().home_dir().ok())
        .map(|d| d.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// Where Wren keeps its own data on this computer (shown in Privacy).
#[tauri::command]
fn data_folder(app: tauri::AppHandle) -> String {
    app.path().app_data_dir().map(|d| d.to_string_lossy().to_string()).unwrap_or_default()
}

// ---------------- accounts ----------------

#[tauri::command]
fn list_accounts(store: State<'_, AccountStore>) -> AccountsView {
    store.view()
}

#[tauri::command]
fn add_account(store: State<'_, AccountStore>, provider: Provider, name: String, existing: Option<bool>) -> Result<Account, String> {
    if existing == Some(true) {
        store.add_existing(provider, &name)
    } else {
        store.add(provider, &name)
    }
}

#[tauri::command]
fn rename_account(store: State<'_, AccountStore>, id: String, name: String) -> Result<(), String> {
    store.rename(&id, &name)
}

/// "Reset Wren": deletes everything Wren keeps on this computer (accounts list,
/// conversations, attachments) and restarts Wren. Accounts Wren created are
/// signed out first so their CLI logins (e.g. macOS Keychain items) go too.
/// This computer's own CLI logins, Antigravity's login and the user's project
/// files (e.g. research reports) are never touched.
#[tauri::command]
async fn reset_wren(app: tauri::AppHandle) -> Result<(), String> {
    app.state::<Manager>().kill_all();
    app.state::<LoginManager>().cancel_all();
    let sign_outs: Vec<(Provider, Vec<(String, String)>)> = {
        let store = app.state::<AccountStore>();
        store
            .added()
            .into_iter()
            .filter(|a| store.dir_for(a).is_some_and(|d| d.exists()))
            .filter_map(|a| store.env_for(&a.id).ok().map(|env| (a.provider, env)))
            .collect()
    };
    for (provider, env) in sign_outs {
        let _ = accounts::sign_out(provider, &env).await;
    }
    let data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    // Only ever Wren's own folder (named after the app id).
    if data.file_name().and_then(|n| n.to_str()) != Some(app.config().identifier.as_str()) {
        return Err(format!("Refusing to delete an unexpected folder: {}", data.display()));
    }
    if data.exists() {
        std::fs::remove_dir_all(&data).map_err(|e| format!("Could not delete Wren's data: {e}"))?;
    }
    app.restart()
}

/// Signs the account out in its CLI (clears the stored login), then removes it.
#[tauri::command]
async fn remove_account(store: State<'_, AccountStore>, id: String) -> Result<(), String> {
    let acc = store.get(&id)?;
    // The computer's own login (is_default) has no folder: only the label goes.
    if let Some(dir) = store.dir_for(&acc)
        && dir.exists() {
            let env = store.env_for(&id).unwrap_or_default();
            let _ = accounts::sign_out(acc.provider, &env).await;
        }
    store.remove(&id)
}

/// Ask the CLI whether this account is signed in, and save the answer.
#[tauri::command]
async fn check_account(store: State<'_, AccountStore>, id: String) -> Result<Account, String> {
    let acc = store.get(&id)?;
    if acc.provider == Provider::Antigravity {
        return Ok(acc);
    }
    let env = store.env_for(&id)?;
    let (status, email) = accounts::check_status(acc.provider, &env).await?;
    let status = if status == AccountStatus::SignedOut && acc.status == AccountStatus::Pending {
        AccountStatus::Pending
    } else {
        status
    };
    store.set_status(&id, status, email)?;
    store.get(&id)
}

/// Antigravity: you finished signing in, so this account is now the active one.
#[tauri::command]
fn set_active_antigravity(store: State<'_, AccountStore>, id: String) -> Result<(), String> {
    store.set_active_antigravity(&id)
}

// ---------------- sign-in terminal ----------------

#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn start_login(
    logins: State<'_, LoginManager>,
    store: State<'_, AccountStore>,
    login_id: String,
    account_id: String,
    device_code: bool,
    cols: u16,
    rows: u16,
    on_event: Channel<LoginEvent>,
) -> Result<(), String> {
    let acc = store.get(&account_id)?;
    // Antigravity has one login per computer: sign-in is allowed for any of
    // its accounts (it becomes the active one afterwards).
    let env = if acc.provider == Provider::Antigravity { vec![] } else { store.env_for(&account_id)? };
    let args = accounts::login_args(acc.provider, device_code);
    logins.start(login_id, acc.provider, args, env, cols, rows, on_event)
}

#[tauri::command]
fn login_input(logins: State<'_, LoginManager>, login_id: String, data: String) -> Result<(), String> {
    logins.input(&login_id, &data)
}

#[tauri::command]
fn login_resize(logins: State<'_, LoginManager>, login_id: String, cols: u16, rows: u16) -> Result<(), String> {
    logins.resize(&login_id, cols, rows)
}

#[tauri::command]
fn cancel_login(logins: State<'_, LoginManager>, login_id: String) {
    logins.cancel(&login_id)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .manage(Manager::default())
        .manage(LoginManager::default())
        .setup(|app| {
            // In-app updates: checks the latest GitHub release (signed with the owner's key).
            #[cfg(desktop)]
            app.handle().plugin(tauri_plugin_updater::Builder::new().build())?;
            let data = app.path().app_data_dir()?;
            app.manage(AccountStore::load(&data));
            app.manage(History::load(&data));
            let store = attachments::Store::new(&data);
            app.state::<Manager>().set_attach_dir(store.dir().to_string_lossy().to_string());
            app.manage(store);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            open_pane,
            update_pane,
            send_message,
            attach_bytes,
            attach_file,
            attachment_preview,
            set_model,
            set_mode,
            answer_approval,
            set_permission,
            save_report,
            plan_limits,
            resume_conversation,
            list_models,
            list_conversations,
            get_conversation,
            save_conversation,
            delete_conversation,
            rename_conversation,
            usage_records,
            new_conversation,
            stop_pane,
            close_pane,
            detect_clis,
            default_folder,
            data_folder,
            list_accounts,
            add_account,
            rename_account,
            remove_account,
            check_account,
            set_active_antigravity,
            start_login,
            login_input,
            login_resize,
            cancel_login,
            reset_wren
        ])
        .build(tauri::generate_context!())
        .expect("error while building Wren");

    app.run(|handle, event| {
        if let RunEvent::Exit = event {
            handle.state::<Manager>().kill_all();
            handle.state::<LoginManager>().cancel_all();
        }
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn percent_decoding() {
        assert_eq!(super::percent_decode("sales%20Q3.xlsx"), "sales Q3.xlsx");
        assert_eq!(super::percent_decode("%E1%9E%80.csv"), "\u{1780}.csv");
        assert_eq!(super::percent_decode("100%"), "100%");
    }
}
