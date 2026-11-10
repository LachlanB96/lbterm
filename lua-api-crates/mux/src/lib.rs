use config::keyassignment::SpawnTabDomain;
use config::lua::mlua::{self, Lua, UserData, UserDataMethods, Value as LuaValue};
use config::lua::{get_or_create_module, get_or_create_sub_module};
use luahelper::impl_lua_conversion_dynamic;
use mlua::UserDataRef;
use mux::domain::{DomainId, SplitSource};
use mux::pane::{Pane, PaneId};
use mux::tab::{SplitDirection, SplitRequest, SplitSize, Tab, TabId};
use mux::window::{Window, WindowId};
use mux::Mux;
use portable_pty::CommandBuilder;
use std::collections::HashMap;
use std::sync::Arc;
use wezterm_dynamic::{FromDynamic, ToDynamic};
use wezterm_term::TerminalSize;

mod domain;
mod pane;
mod tab;
mod window;

pub use domain::MuxDomain;
pub use pane::MuxPane;
pub use tab::MuxTab;
pub use window::MuxWindow;

fn get_mux() -> mlua::Result<Arc<Mux>> {
    Mux::try_get().ok_or_else(|| mlua::Error::external("cannot get Mux!?"))
}

pub fn register(lua: &Lua) -> anyhow::Result<()> {
    let mux_mod = get_or_create_sub_module(lua, "mux")?;

    mux_mod.set(
        "get_active_workspace",
        lua.create_function(|_, _: ()| {
            let mux = get_mux()?;
            Ok(mux.active_workspace())
        })?,
    )?;

    mux_mod.set(
        "get_workspace_names",
        lua.create_function(|_, _: ()| {
            let mux = get_mux()?;
            Ok(mux.iter_workspaces())
        })?,
    )?;

    mux_mod.set(
        "set_active_workspace",
        lua.create_function(|_, workspace: String| {
            let mux = get_mux()?;
            let workspaces = mux.iter_workspaces();
            if workspaces.contains(&workspace) {
                Ok(mux.set_active_workspace(&workspace))
            } else {
                Err(mlua::Error::external(format!(
                    "{:?} is not an existing workspace",
                    workspace
                )))
            }
        })?,
    )?;

    mux_mod.set(
        "rename_workspace",
        lua.create_function(|_, (old_workspace, new_workspace): (String, String)| {
            let mux = get_mux()?;
            mux.rename_workspace(&old_workspace, &new_workspace);
            Ok(())
        })?,
    )?;

    mux_mod.set(
        "get_window",
        lua.create_function(|_, window_id: WindowId| {
            let mux = get_mux()?;
            let window = MuxWindow(window_id);
            let _resolved = window.resolve(&mux)?;
            Ok(window)
        })?,
    )?;

    mux_mod.set(
        "get_pane",
        lua.create_function(|_, pane_id: PaneId| {
            let mux = get_mux()?;
            let pane = MuxPane(pane_id);
            pane.resolve(&mux)?;
            Ok(pane)
        })?,
    )?;

    mux_mod.set(
        "get_tab",
        lua.create_function(|_, tab_id: TabId| {
            let mux = get_mux()?;
            let tab = MuxTab(tab_id);
            tab.resolve(&mux)?;
            Ok(tab)
        })?,
    )?;

    mux_mod.set(
        "spawn_window",
        lua.create_async_function(|_, spawn: SpawnWindow| async move { spawn.spawn().await })?,
    )?;

    mux_mod.set(
        "all_windows",
        lua.create_function(|_, _: ()| {
            let mux = get_mux()?;
            Ok(mux
                .iter_windows()
                .into_iter()
                .map(MuxWindow)
                .collect::<Vec<MuxWindow>>())
        })?,
    )?;

    mux_mod.set(
        "get_domain",
        lua.create_function(|_, domain: LuaValue| {
            let mux = get_mux()?;
            match domain {
                LuaValue::Nil => Ok(Some(MuxDomain(mux.default_domain().domain_id()))),
                LuaValue::String(s) => match s.to_str() {
                    Ok(name) => Ok(mux
                        .get_domain_by_name(name)
                        .map(|dom| MuxDomain(dom.domain_id()))),
                    Err(err) => Err(mlua::Error::external(format!(
                        "invalid domain identifier passed to mux.get_domain: {err:#}"
                    ))),
                },
                LuaValue::Integer(id) => match TryInto::<DomainId>::try_into(id) {
                    Ok(id) => Ok(mux.get_domain(id).map(|dom| MuxDomain(dom.domain_id()))),
                    Err(err) => Err(mlua::Error::external(format!(
                        "invalid domain identifier passed to mux.get_domain: {err:#}"
                    ))),
                },
                _ => Err(mlua::Error::external(
                    "invalid domain identifier passed to mux.get_domain".to_string(),
                )),
            }
        })?,
    )?;

    mux_mod.set(
        "all_domains",
        lua.create_function(|_, _: ()| {
            let mux = get_mux()?;
            Ok(mux
                .iter_domains()
                .into_iter()
                .map(|dom| MuxDomain(dom.domain_id()))
                .collect::<Vec<MuxDomain>>())
        })?,
    )?;

    mux_mod.set(
        "set_default_domain",
        lua.create_function(|_, domain: UserDataRef<MuxDomain>| {
            let mux = get_mux()?;
            let domain = domain.resolve(&mux)?;
            mux.set_default_domain(&domain);
            Ok(())
        })?,
    )?;

    Ok(())
}

#[derive(Debug, Default, FromDynamic, ToDynamic)]
struct CommandBuilderFrag {
    args: Option<Vec<String>>,
    cwd: Option<String>,
    #[dynamic(default)]
    set_environment_variables: HashMap<String, String>,
}

impl CommandBuilderFrag {
    /// The command, when `args` names one, and the folder to start in. LBTerm: the folder goes
    /// on the command and is the spawn's own too. Upstream gave it to the command alone, and the
    /// mux starts a spawn with no folder of its own in the open pane's, over the command's, so
    /// `spawn_tab { args = ..., cwd = ... }` started wherever the open pane was (2026-10-29)
    fn to_command_builder(&self) -> (Option<CommandBuilder>, Option<String>) {
        if let Some(args) = &self.args {
            let mut builder = CommandBuilder::from_argv(args.iter().map(Into::into).collect());
            for (k, v) in self.set_environment_variables.iter() {
                builder.env(k, v);
            }
            if let Some(cwd) = self.cwd.clone() {
                builder.cwd(cwd);
            }
            (Some(builder), self.cwd.clone())
        } else {
            (None, self.cwd.clone())
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn a_command_starts_in_the_folder_it_names() {
        let frag = CommandBuilderFrag {
            args: Some(vec!["pwsh".to_string()]),
            cwd: Some("C:/work".to_string()),
            ..Default::default()
        };
        let (builder, dir) = frag.to_command_builder();
        // The spawn's own folder, which the open pane's doesn't override
        assert_eq!(dir.as_deref(), Some("C:/work"));
        let builder = builder.expect("args name a command");
        assert_eq!(builder.get_cwd().and_then(|d| d.to_str()), Some("C:/work"));
        assert_eq!(builder.get_argv(), &vec![std::ffi::OsString::from("pwsh")]);
        // A folder alone, and neither (the mux finds one: the open pane's)
        let frag = CommandBuilderFrag {
            cwd: Some("C:/work".to_string()),
            ..Default::default()
        };
        assert!(matches!(frag.to_command_builder(), (None, Some(d)) if d == "C:/work"));
        assert!(matches!(CommandBuilderFrag::default().to_command_builder(), (None, None)));
    }

    #[test]
    fn spawn_tab_switches_to_the_new_tab_unless_told_not_to() {
        use std::collections::BTreeMap;
        use wezterm_dynamic::Value;
        let parse = |fields: Vec<(&str, Value)>| {
            let map: BTreeMap<Value, Value> = fields
                .into_iter()
                .map(|(k, v)| (Value::String(k.to_string()), v))
                .collect();
            SpawnTab::from_dynamic(&Value::Object(map.into()), Default::default())
                .expect("a spawn_tab table")
        };
        // As upstream's: the window goes to it
        assert!(parse(vec![]).activate);
        assert!(!parse(vec![("activate", Value::Bool(false))]).activate);
        let t = parse(vec![
            ("activate", Value::Bool(true)),
            ("cwd", Value::String("C:/work".to_string())),
        ]);
        assert!(t.activate);
        assert_eq!(t.cmd_builder.cwd.as_deref(), Some("C:/work"));
    }
}

#[derive(Debug, FromDynamic, ToDynamic)]
enum HandySplitDirection {
    Left,
    Right,
    Top,
    Bottom,
}
impl_lua_conversion_dynamic!(HandySplitDirection);

impl Default for HandySplitDirection {
    fn default() -> Self {
        Self::Right
    }
}

#[derive(Debug, FromDynamic, ToDynamic)]
struct SpawnWindow {
    #[dynamic(default = "spawn_tab_default_domain")]
    domain: SpawnTabDomain,
    width: Option<usize>,
    height: Option<usize>,
    workspace: Option<String>,
    position: Option<config::GuiPosition>,
    #[dynamic(flatten)]
    cmd_builder: CommandBuilderFrag,
}
impl_lua_conversion_dynamic!(SpawnWindow);

fn spawn_tab_default_domain() -> SpawnTabDomain {
    SpawnTabDomain::DefaultDomain
}

impl SpawnWindow {
    async fn spawn(self) -> mlua::Result<(MuxTab, MuxPane, MuxWindow)> {
        let mux = get_mux()?;

        let size = match (self.width, self.height) {
            (Some(cols), Some(rows)) => TerminalSize {
                rows,
                cols,
                ..Default::default()
            },
            _ => config::configuration().initial_size(0, None),
        };

        let (cmd_builder, cwd) = self.cmd_builder.to_command_builder();
        let (tab, pane, window_id) = mux
            .spawn_tab_or_window(
                None,
                self.domain,
                cmd_builder,
                cwd,
                size,
                None,
                self.workspace.unwrap_or_else(|| mux.active_workspace()),
                self.position,
            )
            .await
            .map_err(|e| mlua::Error::external(format!("{:#?}", e)))?;

        Ok((
            MuxTab(tab.tab_id()),
            MuxPane(pane.pane_id()),
            MuxWindow(window_id),
        ))
    }
}

#[derive(Debug, FromDynamic, ToDynamic)]
struct SpawnTab {
    #[dynamic(default)]
    domain: SpawnTabDomain,
    /// LBTerm: false adds the tab without switching the window to it, its server's window on a
    /// mux domain too (SpawnNoActivate, as `lbterm cli spawn --no-activate`): the new-tab
    /// picker's Shift+Enter, from Lua (lbterm/NEW-TAB.md)
    #[dynamic(default = "default_activate")]
    activate: bool,
    #[dynamic(flatten)]
    cmd_builder: CommandBuilderFrag,
}
impl_lua_conversion_dynamic!(SpawnTab);

fn default_activate() -> bool {
    true
}

impl SpawnTab {
    async fn spawn(self, window: &MuxWindow) -> mlua::Result<(MuxTab, MuxPane, MuxWindow)> {
        let mux = get_mux()?;
        let size;
        let pane;

        {
            let window = window.resolve(&mux)?;
            size = window
                .get_tab_at_idx(0)
                .map(|tab| tab.get_size())
                .unwrap_or_else(|| config::configuration().initial_size(0, None));

            pane = window
                .get_active_tab()
                .and_then(|tab| tab.get_active_pane().map(|pane| pane.pane_id()));
        };

        let (cmd_builder, cwd) = self.cmd_builder.to_command_builder();

        let (tab, pane, window_id) = mux
            .spawn_tab_or_window_activating(
                Some(window.0),
                self.domain,
                cmd_builder,
                cwd,
                size,
                pane,
                String::new(),
                None, // optional gui window position
                self.activate,
            )
            .await
            .map_err(|e| mlua::Error::external(format!("{:#?}", e)))?;

        Ok((
            MuxTab(tab.tab_id()),
            MuxPane(pane.pane_id()),
            MuxWindow(window_id),
        ))
    }
}

#[derive(Clone, FromDynamic, ToDynamic)]
struct MuxTabInfo {
    pub index: usize,
    pub is_active: bool,
}
impl_lua_conversion_dynamic!(MuxTabInfo);

#[derive(Clone, FromDynamic, ToDynamic)]
struct MuxPaneInfo {
    /// The topological pane index that can be used to reference this pane
    pub index: usize,
    /// true if this is the active pane at the time the position was computed
    pub is_active: bool,
    /// true if this pane is zoomed
    pub is_zoomed: bool,
    /// The offset from the top left corner of the containing tab to the top
    /// left corner of this pane, in cells.
    pub left: usize,
    /// The offset from the top left corner of the containing tab to the top
    /// left corner of this pane, in cells.
    pub top: usize,
    /// The width of this pane in cells
    pub width: usize,
    pub pixel_width: usize,
    /// The height of this pane in cells
    pub height: usize,
    pub pixel_height: usize,
}
impl_lua_conversion_dynamic!(MuxPaneInfo);
