use std::time::Duration;

use data::{
    AppState,
    config::Config,
    node::{IsValid, NodeType, validate_name},
    settings::AppTheme,
    utils::RemoveElem,
};
use dialogs::Dialog;
use drawer::{Drawer, about};
use graph::GraphWindow;
use hardware::{HardwareBridge, Mode};
use item::items_view;
use message::{ConfigMsg, ModifNodeMsg, SettingsMsg, ToogleMsg};
use node_cache::{NodeC, NodesC};

use crate::{drawer::settings_drawer, graph::graph_window_view};

use cosmic::{
    ApplicationExt, Element,
    app::{
        Core, CosmicFlags, Task,
        context_drawer::{ContextDrawer, context_drawer},
    },
    executor,
    iced::{self, time, window},
    iced_core::Length,
    iced_runtime::Action,
    theme,
    widget::{
        Column, Row, Space,
        toaster::{self, Toast, Toasts},
    },
};

use crate::message::{AppMsg, ControlMsg, CustomTempMsg, FlatMsg, LinearMsg, TargetMsg};

use crate::add_node::add_node_button_view;

#[macro_use]
extern crate log;

#[macro_use]
pub mod localize;

mod add_node;
mod dialogs;
mod drawer;
mod graph;
mod headers;
mod icon;
mod input_line;
mod item;
mod message;
mod my_widgets;
mod node_cache;
mod pick_list_utils;

impl<H: HardwareBridge> CosmicFlags for Flags<H> {
    type SubCommand = String;

    type Args = Vec<String>;
}

pub fn run_ui<H: HardwareBridge + 'static>(app_state: AppState<H>) {
    let settings = cosmic::app::Settings::default()
        .theme(to_cosmic_theme(&app_state.dir_manager.settings().theme));

    let flags = Flags { app_state };

    if let Err(e) = cosmic::app::run::<Ui<H>>(settings, flags) {
        error!("error while running ui: {}", e);
        panic!()
    }
}

struct Flags<H: HardwareBridge> {
    app_state: AppState<H>,
}

struct Ui<H: HardwareBridge> {
    core: Core,
    app_state: AppState<H>,
    current_config_cached: String,
    create_button_expanded: bool,
    choose_config_expanded: bool,
    nodes_c: NodesC,
    is_updating: bool,
    graph_window: Option<GraphWindow>,
    toasts: Toasts<AppMsg>,
    dialog: Option<Dialog>,
    drawer: Option<Drawer>,
}

impl<H: HardwareBridge + 'static> cosmic::Application for Ui<H> {
    type Executor = executor::Default;
    type Message = AppMsg;
    type Flags = Flags<H>;

    const APP_ID: &'static str = utils::APP_ID;

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, flags: Self::Flags) -> (Self, Task<Self::Message>) {
        let app_state = flags.app_state;

        let current_config_cached = app_state
            .dir_manager
            .settings()
            .current_config_text()
            .to_owned();

        let dialog = if cfg!(FAN_CONTROL_FORMAT = "flatpak")
            && app_state.dir_manager.state().show_flatpak_dialog
        {
            Some(Dialog::Flatpak)
        } else {
            None
        };

        let ui_state = Ui {
            nodes_c: NodesC::new(app_state.app_graph.nodes.values()),
            app_state,
            core,
            create_button_expanded: false,
            choose_config_expanded: false,
            current_config_cached,
            is_updating: false,
            graph_window: None,
            toasts: Toasts::new(AppMsg::RemoveToast),
            dialog,
            drawer: None,
        };

        let commands = Task::batch([cosmic::task::message(AppMsg::Tick)]);

        (ui_state, commands)
    }

    fn update(&mut self, message: Self::Message) -> Task<Self::Message> {
        let dir_manager = &mut self.app_state.dir_manager;

        match message {
            AppMsg::Tick => {
                self.update_hardware();
            }

            AppMsg::ModifNode(id, modif_node_msg) => {
                let node = self.app_state.app_graph.get_mut(&id);
                match modif_node_msg {
                    ModifNodeMsg::ChangeHardware(hardware_id) => {
                        let bridge = &mut self.app_state.bridge;

                        match &mut node.node_type {
                            NodeType::Control(i) => {
                                if i.is_valid() {
                                    if let Err(e) = i.set_mode(Mode::Auto, bridge) {
                                        error!(
                                            "Can't set control to auto when removing his hardware ref: {e}."
                                        );
                                    }
                                }

                                i.hardware_id = hardware_id;
                                i.control_h = match &i.hardware_id {
                                    Some(hardware_id) => bridge
                                        .hardware()
                                        .controls
                                        .iter()
                                        .find(|h| &h.hardware_id == hardware_id)
                                        .cloned(),

                                    None => None,
                                }
                            }
                            NodeType::Fan(i) => {
                                i.hardware_id = hardware_id;
                                i.fan_h = match &i.hardware_id {
                                    Some(hardware_id) => bridge
                                        .hardware()
                                        .fans
                                        .iter()
                                        .find(|h| &h.hardware_id == hardware_id)
                                        .cloned(),

                                    None => None,
                                }
                            }
                            NodeType::Temp(i) => {
                                i.hardware_id = hardware_id;
                                i.temp_h = match &i.hardware_id {
                                    Some(hardware_id) => bridge
                                        .hardware()
                                        .temps
                                        .iter()
                                        .find(|h| &h.hardware_id == hardware_id)
                                        .cloned(),

                                    None => None,
                                }
                            }
                            _ => panic!("node have no hardware id"),
                        }
                    }
                    ModifNodeMsg::ReplaceInput(input) => {
                        node.inputs.clear();

                        if let Some(input) = &input {
                            node.inputs.push(input.clone())
                        }

                        let optional_name = match input {
                            Some(input) => Some(input.name),
                            None => None,
                        };
                        match &mut node.node_type {
                            NodeType::Control(i) => i.input = optional_name,
                            NodeType::Graph(i) => i.input = optional_name,
                            NodeType::Linear(i, ..) => i.input = optional_name,
                            NodeType::Target(i, ..) => i.input = optional_name,
                            _ => panic!("node have not exactly one input"),
                        }
                    }
                    ModifNodeMsg::AddInput(input) => {
                        node.inputs.push(input.clone());

                        match &mut node.node_type {
                            NodeType::CustomTemp(i) => i.inputs.push(input.name),
                            _ => panic!("node have not multiple inputs"),
                        }
                    }
                    ModifNodeMsg::RemoveInput(input) => {
                        node.inputs.remove_elem(|i| i.id == input.id);

                        match &mut node.node_type {
                            NodeType::CustomTemp(i) => {
                                i.inputs.remove_elem(|n| n == &input.name);
                            }
                            _ => panic!("node have not multiple inputs"),
                        }
                    }
                    ModifNodeMsg::Control(control_msg) => match control_msg {
                        ControlMsg::Active(is_active) => {
                            let control = node.node_type.unwrap_control_mut();
                            control.active = is_active;
                        }
                    },
                    ModifNodeMsg::CustomTemp(custom_temp_msg) => match custom_temp_msg {
                        CustomTempMsg::Kind(kind) => {
                            let custom_temp = node.node_type.unwrap_custom_temp_mut();
                            custom_temp.kind = kind;
                        }
                    },
                    ModifNodeMsg::Flat(flat_msg) => match flat_msg {
                        FlatMsg::Value(value) => {
                            let flat = node.node_type.unwrap_flat_mut();
                            flat.value = value;
                            node.value = Some(value.into());
                        }
                    },
                    ModifNodeMsg::Linear(linear_msg) => {
                        let linear = node.node_type.unwrap_linear_mut();
                        let linear_c = self.nodes_c.get_mut(&id).node_type_c.unwrap_linear_mut();

                        match linear_msg {
                            LinearMsg::MinTemp(min_temp, cached_value) => {
                                linear.min_temp = min_temp;
                                linear_c.min_temp = cached_value;
                            }
                            LinearMsg::MinSpeed(min_speed, cached_value) => {
                                linear.min_speed = min_speed;
                                linear_c.min_speed = cached_value;
                            }
                            LinearMsg::MaxTemp(max_temp, cached_value) => {
                                linear.max_temp = max_temp;
                                linear_c.max_temp = cached_value;
                            }
                            LinearMsg::MaxSpeed(max_speed, cached_value) => {
                                linear.max_speed = max_speed;
                                linear_c.max_speed = cached_value;
                            }
                        }
                    }
                    ModifNodeMsg::Target(target_msg) => {
                        let target = node.node_type.unwrap_target_mut();
                        let target_c = self.nodes_c.get_mut(&id).node_type_c.unwrap_target_mut();

                        match target_msg {
                            TargetMsg::IdleTemp(idle_temp, cached_value) => {
                                target.idle_temp = idle_temp;
                                target_c.idle_temp = cached_value;
                            }
                            TargetMsg::IdleSpeed(idle_speed, cached_value) => {
                                target.idle_speed = idle_speed;
                                target_c.idle_speed = cached_value;
                            }
                            TargetMsg::LoadTemp(load_temp, cached_value) => {
                                target.load_temp = load_temp;
                                target_c.load_temp = cached_value;
                            }
                            TargetMsg::LoadSpeed(load_speed, cached_value) => {
                                target.load_speed = load_speed;
                                target_c.load_speed = cached_value;
                            }
                        }
                    }
                    ModifNodeMsg::Delete => {
                        match self.app_state.app_graph.remove_node(id) {
                            Some(mut node) => {
                                if let NodeType::Control(control) = &mut node.node_type {
                                    if let Err(e) =
                                        control.set_mode(Mode::Auto, &mut self.app_state.bridge)
                                    {
                                        error!("can't set unactive when removing a control: {}", e);
                                    }
                                }
                            }
                            None => error!("Node was not found when trying to remove it"),
                        }

                        self.nodes_c.remove(&id);
                        self.app_state.app_graph.sanitize_inputs(false)
                    }
                    ModifNodeMsg::Graph(graph_msg) => {
                        let graph = node.node_type.unwrap_graph_mut();
                        let _graph_c = self.nodes_c.get_mut(&id).node_type_c.unwrap_graph_mut();

                        match graph_msg {
                            message::GraphMsg::RemoveCoord(coord) => {
                                graph.remove_coord(&coord);
                            }
                            message::GraphMsg::AddCoord(coord) => {
                                graph.add_coord(coord);
                            }
                            message::GraphMsg::ReplaceCoord { previous, new } => {
                                graph.replace_coord(&previous, new);
                            }
                        }
                    }
                }

                self.app_state.update.set_invalid_root_nodes_to_auto(
                    &mut self.app_state.app_graph.nodes,
                    &self.app_state.app_graph.root_nodes,
                    &mut self.app_state.bridge,
                );
            }

            AppMsg::Settings(settings_msg) => match settings_msg {
                SettingsMsg::Theme(theme) => {
                    dir_manager.update_settings(|settings| {
                        settings.theme = theme;
                    });

                    return cosmic::command::set_theme(to_cosmic_theme(&theme));
                }
                SettingsMsg::UpdateDelay(update_delay) => dir_manager.update_settings(|settings| {
                    settings.update_delay = update_delay;
                }),
            },
            AppMsg::NewNode(node_type_light) => {
                let node = self.app_state.app_graph.create_new_node(node_type_light);
                let node_c = NodeC::new(&node);
                self.nodes_c.insert(node.id, node_c);
                self.app_state.app_graph.insert_node(node);
            }
            AppMsg::Toggle(ui_msg) => match ui_msg {
                ToogleMsg::CreateButton(expanded) => self.create_button_expanded = expanded,
                ToogleMsg::Settings => match &self.drawer {
                    Some(drawer) => match drawer {
                        Drawer::Settings => {
                            self.drawer = None;
                            self.set_show_context(false);
                        }
                        Drawer::About => {
                            self.drawer = Some(Drawer::Settings);
                            self.set_show_context(true);
                        }
                    },
                    None => {
                        self.drawer = Some(Drawer::Settings);
                        self.set_show_context(true);
                    }
                },
                ToogleMsg::ChooseConfig(expanded) => {
                    self.choose_config_expanded = expanded;
                }
                ToogleMsg::NodeContextMenu(id, expanded) => {
                    let node_c = self.nodes_c.get_mut(&id);
                    node_c.context_menu_expanded = expanded;
                }
                ToogleMsg::About => {
                    self.drawer = Some(Drawer::About);
                    self.set_show_context(true)
                }
                ToogleMsg::CloseDrawer => {
                    self.set_show_context(false);
                    self.drawer = None;
                }
            },
            AppMsg::Config(config_msg) => match config_msg {
                ConfigMsg::Save => {
                    let config = Config::from_app_graph(&self.app_state.app_graph);

                    if let Err(e) = dir_manager.save_config(&self.current_config_cached, &config) {
                        error!("can't save config: {}", e);
                    } else {
                        return self
                            .toasts
                            .push(Toast::new(fl!("config_saved")))
                            .map(cosmic::action::app);
                    };
                }
                ConfigMsg::Change(selected) => {
                    self.choose_config_expanded = false;

                    if selected.is_some() {
                        self.app_state.update.set_valid_root_nodes_to_auto(
                            &mut self.app_state.app_graph.nodes,
                            &self.app_state.app_graph.root_nodes,
                            &mut self.app_state.bridge,
                        );
                    }

                    match dir_manager.change_config(selected) {
                        Ok(config) => match config {
                            Some((config_name, config)) => {
                                self.current_config_cached = config_name;
                                self.app_state
                                    .app_graph
                                    .apply_config(config, self.app_state.bridge.hardware());
                                self.nodes_c = NodesC::new(self.app_state.app_graph.nodes.values());

                                self.update_hardware();
                            }
                            None => {
                                self.current_config_cached.clear();
                            }
                        },
                        Err(e) => {
                            error!("can't change config: {}", e);
                        }
                    }
                }
                ConfigMsg::Delete(name) => match dir_manager.remove_config(name) {
                    Ok(is_current_config) => {
                        if is_current_config {
                            self.current_config_cached.clear();
                        }
                    }
                    Err(e) => {
                        error!("can't delete config: {}", e);
                    }
                },
                ConfigMsg::Create(new_name) => {
                    let config = Config::from_app_graph(&self.app_state.app_graph);

                    match dir_manager.create_config(&new_name, &config) {
                        Ok(_) => {
                            self.current_config_cached = new_name;
                        }
                        Err(e) => {
                            error!("can't create config: {}", e);
                        }
                    }
                }
                ConfigMsg::Rename(name) => {
                    self.current_config_cached = name;
                }
            },
            AppMsg::Rename(id, name) => {
                let name_is_valid = validate_name(&self.app_state.app_graph.nodes, &id, &name);

                let node = self.app_state.app_graph.get_mut(&id);
                let node_c = self.nodes_c.get_mut(&id);

                node_c.name.clone_from(&name);
                if name_is_valid {
                    node_c.is_error_name = false;
                    let previous_name = node.name().clone();
                    node.node_type.set_name(name.clone());

                    let node_id = node.id;

                    // find nodes that depend on node.id
                    // change the name in input and item.input
                    for n in self.app_state.app_graph.nodes.values_mut() {
                        if let Some(node_input) = n
                            .inputs
                            .iter_mut()
                            .find(|node_input| node_input.id == node_id)
                        {
                            node_input.name.clone_from(&name);
                            let mut inputs = n.node_type.get_inputs();

                            match inputs.iter().position(|n| n == &previous_name) {
                                Some(index) => {
                                    inputs[index].clone_from(&name);
                                    n.node_type.set_inputs(inputs)
                                }
                                None => {
                                    error!(
                                        "input id found in node inputs but the corresponding name was not found in item input"
                                    )
                                }
                            }
                        }
                    }
                } else {
                    node_c.is_error_name = true;
                }
            }
            AppMsg::GraphWindow(graph_window_msg) => match graph_window_msg {
                graph::GraphWindowMsg::Toogle(node_id) => match node_id {
                    Some(node_id) => {
                        let mut commands = Vec::new();

                        if let Some(graph_window) = &self.graph_window {
                            let command = cosmic::iced::runtime::task::effect(Action::Window(
                                window::Action::Close(graph_window.window_id),
                            ));
                            commands.push(command);
                        }

                        let (new_id, command) =
                            cosmic::iced::runtime::window::open(graph::window_settings());

                        self.graph_window = Some(GraphWindow {
                            window_id: new_id,
                            node_id,
                            temp_c: String::new(),
                            percent_c: String::new(),
                        });

                        commands.push(command.map(|_| cosmic::action::none()));

                        return Task::batch(commands);
                    }
                    None => {
                        if let Some(graph_window) = &self.graph_window {
                            return cosmic::iced::runtime::task::effect(Action::Window(
                                window::Action::Close(graph_window.window_id),
                            ));
                        }
                    }
                },
                graph::GraphWindowMsg::ChangeTemp(temp) => {
                    if let Some(graph_window) = &mut self.graph_window {
                        graph_window.temp_c = temp;
                    }
                }
                graph::GraphWindowMsg::ChangePercent(percent) => {
                    if let Some(graph_window) = &mut self.graph_window {
                        graph_window.percent_c = percent;
                    }
                }
            },
            AppMsg::RemoveToast(pos) => {
                self.toasts.remove(pos);
            }
            AppMsg::Dialog(dialog_msg) => {
                return Dialog::update(self, dialog_msg).map(cosmic::action::app);
            }
            AppMsg::OpenUrl(url) => {
                if let Err(e) = open::that(url.as_str()) {
                    error!("{e}");
                }
            }
        }

        Task::none()
    }

    fn view(&self) -> Element<Self::Message> {
        let app_state = &self.app_state;
        let app_graph = &app_state.app_graph;

        let content = items_view(&app_graph.nodes, &self.nodes_c, app_state.bridge.hardware());

        let floating_button = Column::new()
            .push(Space::new(0.0, Length::Fill))
            .push(add_node_button_view(self.create_button_expanded));

        let app = Row::new().push(content).push(floating_button);

        toaster::toaster(&self.toasts, app)
    }

    fn header_start(&self) -> Vec<Element<Self::Message>> {
        headers::header_start()
    }

    fn header_center(&self) -> Vec<Element<Self::Message>> {
        headers::header_center(
            &self.app_state,
            &self.current_config_cached,
            self.choose_config_expanded,
        )
    }

    fn header_end(&self) -> Vec<Element<Self::Message>> {
        headers::header_end()
    }

    fn context_drawer(&self) -> Option<ContextDrawer<Self::Message>> {
        self.drawer.as_ref().map(|drawer| match drawer {
            Drawer::Settings => context_drawer(
                settings_drawer(&self.app_state.dir_manager),
                AppMsg::Toggle(ToogleMsg::CloseDrawer),
            )
            .title(fl!("settings")),
            Drawer::About => {
                context_drawer(about(), AppMsg::Toggle(ToogleMsg::CloseDrawer)).title(fl!("about"))
            }
        })
    }

    fn subscription(&self) -> iced::Subscription<Self::Message> {
        time::every(Duration::from_millis(
            self.app_state.dir_manager.settings().update_delay,
        ))
        .map(|_| AppMsg::Tick)

        //cosmic::iced_futures::Subscription::none()
    }

    fn on_app_exit(&mut self) -> Option<Self::Message> {
        if let Err(e) = self.app_state.bridge.shutdown() {
            error!("shutdown hardware: {}", e);
        }

        let runtime_config = Config::from_app_graph(&self.app_state.app_graph);

        if match self.app_state.dir_manager.get_config() {
            Some(saved_config) => saved_config != runtime_config,
            None => true,
        } {
            if let Err(err) = self
                .app_state
                .dir_manager
                .save_config_cached(&runtime_config)
            {
                error!("{err}")
            } else {
                info!("cached config saved successfully");
            }
        } else if let Err(err) = self.app_state.dir_manager.remove_config_cached() {
            error!("{err}")
        }

        None
    }

    fn view_window(&self, id: window::Id) -> Element<Self::Message> {
        if let Some(graph_window) = &self.graph_window {
            if graph_window.window_id == id {
                let graph = self
                    .app_state
                    .app_graph
                    .get(&graph_window.node_id)
                    .node_type
                    .unwrap_graph_ref();

                return graph_window_view(graph_window, graph);
            }
        }

        panic!("no view for window {id:?}");
    }

    fn dialog(&self) -> Option<Element<Self::Message>> {
        self.dialog.as_ref().map(|dialog| dialog.view())
    }
}

// todo: re enable when is "work" on flatpak
// currently, light theme will be the dark one
// fn to_cosmic_theme(theme: &AppTheme) -> theme::Theme {
//     match theme {
//         AppTheme::Dark => {
//             let mut t = theme::system_dark();
//             t.theme_type.prefer_dark(Some(true));
//             t
//         }
//         AppTheme::Light => {
//             let mut t = theme::system_light();
//             t.theme_type.prefer_dark(Some(false));
//             t
//         }
//         AppTheme::System => theme::system_preference(),
//     }
// }

fn to_cosmic_theme(theme: &AppTheme) -> theme::Theme {
    match theme {
        AppTheme::Dark => theme::Theme::dark(),
        AppTheme::Light => theme::Theme::light(),
        AppTheme::HighContrastDark => theme::Theme::dark_hc(),
        AppTheme::HighContrastLight => theme::Theme::light_hc(),
        AppTheme::System => theme::system_preference(),
    }
}

impl<H: HardwareBridge> Ui<H> {
    fn update_hardware(&mut self) {
        if self.is_updating {
            warn!("An update is already processing: skipping that one.");
            return;
        }

        self.is_updating = true;

        if let Err(e) = self.app_state.bridge.update() {
            error!("{}", e);
            self.is_updating = false;
            return;
        }
        if let Err(e) = self.app_state.update.all(
            &mut self.app_state.app_graph.nodes,
            &mut self.app_state.bridge,
        ) {
            error!("{}", e);
            self.is_updating = false;
            return;
        }

        if let Err(e) = self.app_state.update.nodes_which_update_can_change(
            &mut self.app_state.app_graph.nodes,
            &mut self.app_state.bridge,
        ) {
            error!("{}", e);
            self.is_updating = false;
            return;
        }
        self.is_updating = false;
    }
}
