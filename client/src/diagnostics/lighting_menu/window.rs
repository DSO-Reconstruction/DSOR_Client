//! The Lighting Menu window (from the 2018 menu's design, PR #5): a
//! draggable, resizable panel with a sidebar of pages, an opacity and colour
//! theme for its surfaces, and scrolling page bodies. The pages' controls live
//! beside it; this module only owns the window.

use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use bevy::window::PrimaryWindow;



#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum MenuPage {
    #[default]
    Overview,
    Lighting,
    Regions,
    Atmosphere,
    Presets,
}

#[derive(Component)]
struct MenuNav(MenuPage);

#[derive(Component)]
struct MenuPageBody(MenuPage);
#[derive(Component)]
pub(super) struct MenuRoot;

#[derive(Component)]
struct MenuAccentSurface;

#[derive(Component, Clone, Copy)]
struct MenuOpacityStep(f32);

#[derive(Component, Clone, Copy)]
struct MenuThemeChoice(MenuTheme);

#[derive(Component)]
struct MenuOpacityLabel;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum MenuTheme {
    #[default]
    Slate,
    Red,
    Blue,
    Green,
}

impl MenuTheme {
    fn accent(self) -> (f32, f32, f32) {
        match self {
            Self::Slate => (0.82, 0.64, 0.34),
            Self::Red => (0.92, 0.18, 0.24),
            Self::Blue => (0.20, 0.53, 0.98),
            Self::Green => (0.18, 0.76, 0.42),
        }
    }

    fn surface_tint(self) -> ((f32, f32, f32), f32) {
        match self {
            Self::Slate => ((0.055, 0.067, 0.09), 0.0),
            Self::Red => ((0.32, 0.025, 0.045), 0.36),
            Self::Blue => ((0.025, 0.085, 0.32), 0.36),
            Self::Green => ((0.025, 0.20, 0.085), 0.36),
        }
    }
}

#[derive(Resource, Clone, Copy)]
struct MenuAppearance {
    opacity: f32,
    theme: MenuTheme,
}

impl Default for MenuAppearance {
    fn default() -> Self {
        Self {
            opacity: 0.8,
            theme: MenuTheme::Slate,
        }
    }
}

#[derive(Component)]
struct MenuDragHandle;

#[derive(Component, Clone, Copy)]
struct MenuResizeHandle(MenuResizeEdge);

#[derive(Clone, Copy, Debug)]
enum MenuResizeEdge {
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Default)]
struct MenuWindowState {
    dragging: bool,
    resizing: bool,
    drag_offset: Vec2,
    resize_start_cursor: Vec2,
    resize_start_size: Vec2,
    resize_start_position: Vec2,
    resize_edge: Option<MenuResizeEdge>,
}

fn update_menu_appearance(
    opacity_buttons: Query<
        (&Interaction, &MenuOpacityStep),
        Changed<Interaction>,
    >,
    theme_buttons: Query<
        (&Interaction, &MenuThemeChoice),
        Changed<Interaction>,
    >,
    mut opacity_labels: Query<&mut Text, With<MenuOpacityLabel>>,
    roots: Query<Entity, With<MenuRoot>>,
    children: Query<&Children>,
    nav_components: Query<&MenuNav>,
    page_bodies: Query<(&MenuPageBody, &Node)>,
    accent_components: Query<Entity, With<MenuAccentSurface>>,
    mut backgrounds: Query<&mut BackgroundColor>,
    appearance: Option<ResMut<MenuAppearance>>,
    mut original_colors: Local<Vec<(Entity, Color)>>,
) {
    let Some(mut appearance) = appearance else {
        return;
    };

    for (interaction, step) in &opacity_buttons {
        if *interaction == Interaction::Pressed {
            appearance.opacity = (appearance.opacity + step.0).clamp(0.4, 1.0);
        }
    }

    for (interaction, choice) in &theme_buttons {
        if *interaction == Interaction::Pressed {
            appearance.theme = choice.0;
        }
    }

    let opacity_text = format!("{}%", (appearance.opacity * 100.0).round() as u32);
    for mut label in &mut opacity_labels {
        if label.0 != opacity_text {
            label.0 = opacity_text.clone();
        }
    }

    let mut stack: Vec<Entity> = roots.iter().collect();
    let mut entities_to_style = Vec::new();

    while let Some(entity) = stack.pop() {
        entities_to_style.push(entity);
        if let Ok(kids) = children.get(entity) {
            stack.extend(kids.iter());
        }
    }

    let (tint, mix) = appearance.theme.surface_tint();
    let accent = appearance.theme.accent();

    for entity in entities_to_style {
        let Ok(mut background) = backgrounds.get_mut(entity) else {
            continue;
        };

        let base_color = if let Some((_, color)) = original_colors
            .iter()
            .find(|(saved_entity, _)| *saved_entity == entity)
        {
            *color
        } else {
            let color = background.0;
            original_colors.push((entity, color));
            color
        };

        let base = base_color.to_srgba();

        let (r, g, b) = if let Ok(nav) = nav_components.get(entity) {
            let selected = page_bodies.iter().any(|(body, node)| {
                body.0 == nav.0 && node.display != bevy::ui::Display::None
            });
            let idle = (0.12, 0.14, 0.19);
            if selected {
                (
                    idle.0 + (accent.0 - idle.0) * 0.28,
                    idle.1 + (accent.1 - idle.1) * 0.28,
                    idle.2 + (accent.2 - idle.2) * 0.28,
                )
            } else {
                idle
            }
        } else if accent_components.get(entity).is_ok() {
            accent
        } else {
            (
                base.red + (tint.0 - base.red) * mix,
                base.green + (tint.1 - base.green) * mix,
                base.blue + (tint.2 - base.blue) * mix,
            )
        };

        let styled = Color::srgba(
            r.clamp(0.0, 1.0),
            g.clamp(0.0, 1.0),
            b.clamp(0.0, 1.0),
            (base.alpha * appearance.opacity).clamp(0.0, 1.0),
        );

        if background.0 != styled {
            background.0 = styled;
        }
    }
}

fn update_menu_window(
    windows: Query<&Window, With<PrimaryWindow>>,
    mouse: Res<ButtonInput<MouseButton>>,
    drag_handles: Query<&Interaction, (With<MenuDragHandle>, Changed<Interaction>)>,
    resize_handles: Query<(&Interaction, &MenuResizeHandle), Changed<Interaction>>,
    mut panels: Query<&mut Node, With<MenuRoot>>,
    mut state: Local<MenuWindowState>,
) {
    let drag_started = drag_handles
        .iter()
        .any(|interaction| *interaction == Interaction::Pressed);

    let resize_started = resize_handles.iter().find_map(|(interaction, handle)| {
        (*interaction == Interaction::Pressed).then_some(handle.0)
    });

    if !mouse.pressed(MouseButton::Left) {
        state.dragging = false;
        state.resizing = false;
        state.resize_edge = None;
        return;
    }

    let Ok(window) = windows.single() else { return };
    let Some(cursor) = window.cursor_position() else { return };
    let Ok(mut node) = panels.single_mut() else { return };

    let px = |value: Val, fallback: f32| match value {
        Val::Px(value) => value,
        _ => fallback,
    };

    if drag_started {
        state.dragging = true;
        state.resizing = false;
        state.resize_edge = None;
        state.drag_offset = cursor - Vec2::new(
            px(node.left, 40.0),
            px(node.top, 40.0),
        );
    }

    if let Some(edge) = resize_started {
        state.resizing = true;
        state.dragging = false;
        state.resize_edge = Some(edge);
        state.resize_start_cursor = cursor;
        state.resize_start_position = Vec2::new(
            px(node.left, 40.0),
            px(node.top, 40.0),
        );
        state.resize_start_size = Vec2::new(
            px(node.width, 900.0),
            px(node.height, 650.0),
        );
    }

    if state.dragging {
        let width = px(node.width, 900.0);
        let height = px(node.height, 650.0);

        node.left = Val::Px(
            (cursor.x - state.drag_offset.x)
                .clamp(0.0, (window.width() - width).max(0.0)),
        );
        node.top = Val::Px(
            (cursor.y - state.drag_offset.y)
                .clamp(0.0, (window.height() - height).max(0.0)),
        );
    }

    if state.resizing {
        let Some(edge) = state.resize_edge else { return };
        let delta = cursor - state.resize_start_cursor;
        let start_left = state.resize_start_position.x;
        let start_top = state.resize_start_position.y;
        let start_width = state.resize_start_size.x;
        let start_height = state.resize_start_size.y;

        let fixed_right = start_left + start_width;
        let fixed_bottom = start_top + start_height;

        let change_left = matches!(
            edge,
            MenuResizeEdge::Left
                | MenuResizeEdge::TopLeft
                | MenuResizeEdge::BottomLeft
        );
        let change_right = matches!(
            edge,
            MenuResizeEdge::Right
                | MenuResizeEdge::TopRight
                | MenuResizeEdge::BottomRight
        );
        let change_top = matches!(
            edge,
            MenuResizeEdge::Top
                | MenuResizeEdge::TopLeft
                | MenuResizeEdge::TopRight
        );
        let change_bottom = matches!(
            edge,
            MenuResizeEdge::Bottom
                | MenuResizeEdge::BottomLeft
                | MenuResizeEdge::BottomRight
        );

        let mut left = start_left;
        let mut top = start_top;
        let mut width = start_width;
        let mut height = start_height;

        if change_left {
            let max_width = (fixed_right - 12.0).max(520.0);
            width = (start_width - delta.x).clamp(520.0, max_width);
            left = fixed_right - width;
        } else if change_right {
            let max_width = (window.width() - start_left - 12.0).max(520.0);
            width = (start_width + delta.x).clamp(520.0, max_width);
        }

        if change_top {
            let max_height = (fixed_bottom - 12.0).max(420.0);
            height = (start_height - delta.y).clamp(420.0, max_height);
            top = fixed_bottom - height;
        } else if change_bottom {
            let max_height = (window.height() - start_top - 12.0).max(420.0);
            height = (start_height + delta.y).clamp(420.0, max_height);
        }

        node.width = Val::Px(width);
        node.height = Val::Px(height);
        node.left = Val::Px(left.clamp(0.0, (window.width() - width).max(0.0)));
        node.top = Val::Px(top.clamp(0.0, (window.height() - height).max(0.0)));
    }
}


pub(super) fn menu_text(
    text: impl Into<String>,
    size: f32,
    color: Color,
) -> (Text, TextFont, TextColor) {
    (
        Text::new(text),
        TextFont {
            font_size: bevy::text::FontSize::Px(size),
            ..default()
        },
        TextColor(color),
    )
}


fn scroll_menu_pages(
    mut mouse_wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    mut pages: Query<
        (&RelativeCursorPosition, &mut ScrollPosition, &ComputedNode),
        With<MenuPageBody>,
    >,
) {
    for wheel in mouse_wheel.read() {
        let delta_y = match wheel.unit {
            bevy::input::mouse::MouseScrollUnit::Line => wheel.y * 32.0,
            bevy::input::mouse::MouseScrollUnit::Pixel => wheel.y,
        };

        if delta_y == 0.0 {
            continue;
        }

        for (cursor, mut position, computed) in &mut pages {
            if !cursor.cursor_over {
                continue;
            }

            let max_y = (computed.content_size().y - computed.size().y).max(0.0)
                * computed.inverse_scale_factor;

            position.0.y = (position.0.y - delta_y).clamp(0.0, max_y);
        }
    }
}


fn update_menu_navigation(
    buttons: Query<(&Interaction, &MenuNav), Changed<Interaction>>,
    mut nav_styles: Query<(&MenuNav, &mut BackgroundColor)>,
    mut pages: Query<(&MenuPageBody, &mut Node)>,
) {
    let selected = buttons.iter().find_map(|(interaction, nav)| {
        (*interaction == Interaction::Pressed).then_some(nav.0)
    });
    let Some(selected) = selected else {
        return;
    };

    for (nav, mut background) in &mut nav_styles {
        background.0 = if nav.0 == selected {
            Color::srgb(0.29, 0.25, 0.18)
        } else {
            Color::srgb(0.12, 0.14, 0.19)
        };
    }
    for (page, mut node) in &mut pages {
        node.display = if page.0 == selected {
            bevy::ui::Display::Flex
        } else {
            bevy::ui::Display::None
        };
    }
}


fn spawn_menu(commands: &mut Commands, starts_open: bool) {
    commands.insert_resource(MenuAppearance::default());
    commands.spawn((
        MenuRoot,
        // Clicks, drags and the wheel over the window stay out of the world.
        crate::ui::input::BlocksWorldInput,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(40.0),
            top: Val::Px(40.0),
            width: Val::Px(900.0),
            height: Val::Px(650.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(12.0),
            padding: UiRect::new(Val::Px(14.0), Val::Px(14.0), Val::Px(0.0), Val::Px(30.0)),
            ..default()
        },
        ScrollPosition::default(),
        RelativeCursorPosition::default(),
        BackgroundColor(Color::srgb(0.055, 0.067, 0.09)),
        if starts_open { Visibility::Inherited } else { Visibility::Hidden },
    ))
    .with_children(|panel| {
        panel.spawn((
            Button,
            MenuDragHandle,
            Node {
                width: Val::Percent(100.0),
                min_height: Val::Px(58.0),
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                ..default()
            },
        ))
        .with_children(|header| {
            header.spawn((
                Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(12.0),
                    align_items: AlignItems::Center,
                    ..default()
                },
            ))
            .with_children(|brand| {
                brand.spawn((
                    Node {
                        width: Val::Px(46.0),
                        height: Val::Px(46.0),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    MenuAccentSurface,
                    BackgroundColor(Color::srgb(0.82, 0.64, 0.34)),
                ))
                .with_children(|mark| {
                    mark.spawn((
                        Text::new("D"),
                        TextFont {
                            font_size: bevy::text::FontSize::Px(24.0),
                            ..default()
                        },
                        TextColor(Color::srgb(0.07, 0.08, 0.10)),
                    ));
                });

                brand.spawn(Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(3.0),
                    ..default()
                })
                .with_children(|title| {
                    title.spawn((
                        Text::new("LIGHTING MENU"),
                        TextFont {
                            font_size: bevy::text::FontSize::Px(22.0),
                            ..default()
                        },
                        TextColor(Color::srgb(0.94, 0.95, 0.97)),
                    ));
                    title.spawn((
                        Text::new("DSOR  /  WORLD ENVIRONMENT EDITOR"),
                        TextFont {
                            font_size: bevy::text::FontSize::Px(11.0),
                            ..default()
                        },
                        TextColor(Color::srgb(0.59, 0.65, 0.74)),
                    ));
                });
            });

            header.spawn((
                Node {
                    padding: UiRect::axes(Val::Px(12.0), Val::Px(7.0)),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(Color::srgb(0.20, 0.18, 0.14)),
            ))
            .with_children(|tag| {
                tag.spawn((
                    Text::new("PREVIEW  /  DRAG HEADER TO MOVE"),
                    TextFont {
                        font_size: bevy::text::FontSize::Px(11.0),
                        ..default()
                    },
                    TextColor(Color::srgb(0.91, 0.73, 0.43)),
                ));
            });
        });

        panel.spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(1.0),
                ..default()
            },
            BackgroundColor(Color::srgb(0.20, 0.23, 0.29)),
        ));

        panel.spawn((
            Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(12.0),
                ..default()
            },
        ))
        .with_children(|workspace| {
            workspace.spawn((
                Node {
                    width: Val::Px(192.0),
                    flex_shrink: 0.0,
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(7.0),
                    padding: UiRect::all(Val::Px(11.0)),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.085, 0.10, 0.135)),
            ))
            .with_children(|sidebar| {
                sidebar.spawn((
                    Text::new("WORKSPACE"),
                    TextFont {
                        font_size: bevy::text::FontSize::Px(11.0),
                        ..default()
                    },
                    TextColor(Color::srgb(0.58, 0.65, 0.75)),
                ));

                for (page, number, label) in [
                    (MenuPage::Overview, "01", "Overview"),
                    (MenuPage::Lighting, "02", "Lighting"),
                    (MenuPage::Regions, "03", "Regions"),
                    (MenuPage::Atmosphere, "04", "Atmosphere"),
                    (MenuPage::Presets, "05", "Presets"),
                ] {
                    let selected = page == MenuPage::Overview;
                    sidebar.spawn((
                        Button,
                        MenuNav(page),
                        Node {
                            width: Val::Percent(100.0),
                            min_height: Val::Px(43.0),
                            flex_direction: FlexDirection::Row,
                            column_gap: Val::Px(10.0),
                            padding: UiRect::axes(Val::Px(10.0), Val::Px(8.0)),
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        BackgroundColor(if selected {
                            Color::srgb(0.29, 0.25, 0.18)
                        } else {
                            Color::srgb(0.12, 0.14, 0.19)
                        }),
                    ))
                    .with_children(|button| {
                        button.spawn((
                            Text::new(number),
                            TextFont {
                                font_size: bevy::text::FontSize::Px(11.0),
                                ..default()
                            },
                            TextColor(Color::srgb(0.83, 0.68, 0.43)),
                        ));
                        button.spawn((
                            Text::new(label),
                            TextFont {
                                font_size: bevy::text::FontSize::Px(14.0),
                                ..default()
                            },
                            TextColor(Color::srgb(0.91, 0.93, 0.96)),
                        ));
                    });
                }

                sidebar.spawn(Node {
                    flex_grow: 1.0,
                    ..default()
                });

                sidebar.spawn((
                    Text::new("F2  /  TOGGLE MENU"),
                    TextFont {
                        font_size: bevy::text::FontSize::Px(10.0),
                        ..default()
                    },
                    TextColor(Color::srgb(0.57, 0.63, 0.72)),
                ));
            });

            workspace.spawn((
                Node {
                    flex_grow: 1.0,
                    min_width: Val::Px(0.0),
                    min_height: Val::Px(0.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(8.0),
                    padding: UiRect::all(Val::Px(12.0)),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.075, 0.09, 0.12)),
            ))
            .with_children(|main| {
                // OVERVIEW: a compact dashboard, kept inside its own scrollable page.
                main.spawn((
                    MenuPageBody(MenuPage::Overview),
                    Node {
                        width: Val::Percent(100.0),
                        flex_grow: 1.0,
                        min_width: Val::Px(0.0),
                        min_height: Val::Px(0.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(9.0),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                    ScrollPosition::default(),
                    RelativeCursorPosition::default(),
                ))
                .with_children(|page| {
                    page.spawn(menu_text(
                        "Environment overview",
                        25.0,
                        Color::srgb(0.94, 0.95, 0.97),
                    ));
                    page.spawn(menu_text(
                        "A starting point for the map's lighting and environment tools.",
                        13.0,
                        Color::srgb(0.64, 0.70, 0.79),
                    ));

                    page.spawn((
                        Node {
                            width: Val::Percent(100.0),
                            flex_direction: FlexDirection::Column,
                            row_gap: Val::Px(9.0),
                            padding: UiRect::all(Val::Px(12.0)),
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.095, 0.115, 0.15)),
                    ))
                    .with_children(|appearance| {
                        appearance.spawn(menu_text(
                            "PANEL APPEARANCE",
                            12.0,
                            Color::srgb(0.91, 0.73, 0.43),
                        ));
                        appearance.spawn(menu_text(
                            "Make the menu more transparent or choose a different color theme.",
                            12.0,
                            Color::srgb(0.72, 0.77, 0.84),
                        ));

                        appearance.spawn(Node {
                            width: Val::Percent(100.0),
                            flex_direction: FlexDirection::Row,
                            column_gap: Val::Px(7.0),
                            align_items: AlignItems::Center,
                            ..default()
                        })
                        .with_children(|row| {
                            row.spawn(menu_text(
                                "Opacity",
                                12.0,
                                Color::srgb(0.91, 0.93, 0.96),
                            ));

                            row.spawn((
                                Button,
                                MenuOpacityStep(-0.1),
                                Node {
                                    width: Val::Px(32.0),
                                    min_height: Val::Px(30.0),
                                    flex_shrink: 0.0,
                                    justify_content: JustifyContent::Center,
                                    align_items: AlignItems::Center,
                                    ..default()
                                },
                                BackgroundColor(Color::srgb(0.20, 0.23, 0.29)),
                            ))
                            .with_children(|button| {
                                button.spawn((
                                    Text::new("-"),
                                    TextFont {
                                        font_size: bevy::text::FontSize::Px(15.0),
                                        ..default()
                                    },
                                    TextColor(Color::srgb(0.94, 0.95, 0.97)),
                                ));
                            });

                            row.spawn((
                                MenuOpacityLabel,
                                Text::new("80%"),
                                TextFont {
                                    font_size: bevy::text::FontSize::Px(12.0),
                                    ..default()
                                },
                                TextColor(Color::srgb(0.94, 0.95, 0.97)),
                                Node {
                                    width: Val::Px(60.0),
                                    justify_content: JustifyContent::Center,
                                    ..default()
                                },
                            ));

                            row.spawn((
                                Button,
                                MenuOpacityStep(0.1),
                                Node {
                                    width: Val::Px(32.0),
                                    min_height: Val::Px(30.0),
                                    flex_shrink: 0.0,
                                    justify_content: JustifyContent::Center,
                                    align_items: AlignItems::Center,
                                    ..default()
                                },
                                BackgroundColor(Color::srgb(0.20, 0.23, 0.29)),
                            ))
                            .with_children(|button| {
                                button.spawn((
                                    Text::new("+"),
                                    TextFont {
                                        font_size: bevy::text::FontSize::Px(15.0),
                                        ..default()
                                    },
                                    TextColor(Color::srgb(0.94, 0.95, 0.97)),
                                ));
                            });
                        });

                        for themes in [
                            [
                                (MenuTheme::Slate, "Slate"),
                                (MenuTheme::Red, "Red"),
                            ],
                            [
                                (MenuTheme::Blue, "Blue"),
                                (MenuTheme::Green, "Green"),
                            ],
                        ] {
                            appearance.spawn(Node {
                                width: Val::Percent(100.0),
                                flex_direction: FlexDirection::Row,
                                column_gap: Val::Px(7.0),
                                ..default()
                            })
                            .with_children(|row| {
                                for (theme, label) in themes {
                                    row.spawn((
                                        Button,
                                        MenuThemeChoice(theme),
                                        Node {
                                            flex_grow: 1.0,
                                            min_width: Val::Px(0.0),
                                            min_height: Val::Px(31.0),
                                            padding: UiRect::axes(
                                                Val::Px(8.0),
                                                Val::Px(5.0),
                                            ),
                                            justify_content: JustifyContent::Center,
                                            align_items: AlignItems::Center,
                                            ..default()
                                        },
                                        BackgroundColor(Color::srgb(0.20, 0.23, 0.29)),
                                    ))
                                    .with_children(|button| {
                                        button.spawn((
                                            Text::new(label),
                                            TextFont {
                                                font_size: bevy::text::FontSize::Px(12.0),
                                                ..default()
                                            },
                                            TextColor(Color::srgb(0.94, 0.95, 0.97)),
                                        ));
                                    });
                                }
                            });
                        }

                        appearance.spawn(menu_text(
                            "Opacity changes menu surfaces only. Themes recolor the panel and active page highlight, not game lighting or presets.",
                            11.0,
                            Color::srgb(0.64, 0.70, 0.79),
                        ));
                    });

                    for (number, heading, description, status) in [
                        ("01", "GLOBAL LIGHTING", "The overall look of the map.", "LIGHTING"),
                        ("02", "AMBIENCE REGIONS", "Local areas, bounds and transitions.", "REGIONS"),
                        ("03", "PRESETS", "Saved looks and configurations.", "SAVED LOOKS"),
                    ] {
                        page.spawn((
                            Node {
                                width: Val::Percent(100.0),
                                min_height: Val::Px(66.0),
                                flex_direction: FlexDirection::Row,
                                column_gap: Val::Px(11.0),
                                align_items: AlignItems::Center,
                                padding: UiRect::all(Val::Px(10.0)),
                                ..default()
                            },
                            BackgroundColor(Color::srgb(0.12, 0.145, 0.19)),
                        ))
                        .with_children(|card| {
                            card.spawn((
                                Node {
                                    width: Val::Px(38.0),
                                    height: Val::Px(38.0),
                                    justify_content: JustifyContent::Center,
                                    align_items: AlignItems::Center,
                                    flex_shrink: 0.0,
                                    ..default()
                                },
                                BackgroundColor(Color::srgb(0.25, 0.22, 0.17)),
                            ))
                            .with_children(|number_box| {
                                number_box.spawn(menu_text(
                                    number,
                                    13.0,
                                    Color::srgb(0.91, 0.73, 0.43),
                                ));
                            });

                            card.spawn(Node {
                                flex_grow: 1.0,
                                min_width: Val::Px(0.0),
                                flex_direction: FlexDirection::Column,
                                row_gap: Val::Px(4.0),
                                ..default()
                            })
                            .with_children(|details| {
                                details.spawn(menu_text(
                                    heading,
                                    12.0,
                                    Color::srgb(0.91, 0.73, 0.43),
                                ));
                                details.spawn(menu_text(
                                    description,
                                    13.0,
                                    Color::srgb(0.83, 0.86, 0.91),
                                ));
                            });

                            card.spawn(menu_text(
                                status,
                                10.0,
                                Color::srgb(0.61, 0.69, 0.79),
                            ));
                        });
                    }

                    page.spawn((
                        Node {
                            width: Val::Percent(100.0),
                            min_height: Val::Px(58.0),
                            flex_direction: FlexDirection::Column,
                            row_gap: Val::Px(5.0),
                            padding: UiRect::all(Val::Px(10.0)),
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.095, 0.115, 0.15)),
                    ))
                    .with_children(|note| {
                        note.spawn(menu_text(
                            "DESIGN PREVIEW",
                            11.0,
                            Color::srgb(0.91, 0.73, 0.43),
                        ));
                        note.spawn(menu_text(
                            "All values shown in this prototype are examples. Real controls will be connected after the layout is settled.",
                            12.0,
                            Color::srgb(0.72, 0.77, 0.84),
                        ));
                    });
                });

                // Each following page is a visual mockup, not a live editor yet.
                for page_id in [
                    MenuPage::Lighting,
                    MenuPage::Regions,
                    MenuPage::Atmosphere,
                    MenuPage::Presets,
                ] {
                    main.spawn((
                        MenuPageBody(page_id),
                        Node {
                            display: bevy::ui::Display::None,
                            width: Val::Percent(100.0),
                            flex_grow: 1.0,
                            min_width: Val::Px(0.0),
                            min_height: Val::Px(0.0),
                            flex_direction: FlexDirection::Column,
                            row_gap: Val::Px(10.0),
                            overflow: Overflow::scroll_y(),
                            ..default()
                        },
                        ScrollPosition::default(),
                    RelativeCursorPosition::default(),
                    ))
                    .with_children(|page| {
                        page_contents(page, page_id);
                    });
                }

            });
        });

        // Transparent resize hitboxes. No visible marks overlap the menu content.
        for (edge, node) in [
            (
                MenuResizeEdge::Left,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    top: Val::Px(58.0),
                    bottom: Val::Px(30.0),
                    width: Val::Px(18.0),
                    ..default()
                },
            ),
            (
                MenuResizeEdge::Right,
                Node {
                    position_type: PositionType::Absolute,
                    right: Val::Px(0.0),
                    top: Val::Px(58.0),
                    bottom: Val::Px(30.0),
                    width: Val::Px(18.0),
                    ..default()
                },
            ),
            (
                MenuResizeEdge::Top,
                Node {
                    position_type: PositionType::Absolute,
                    top: Val::Px(0.0),
                    left: Val::Px(18.0),
                    right: Val::Px(18.0),
                    height: Val::Px(14.0),
                    ..default()
                },
            ),
            (
                MenuResizeEdge::Bottom,
                Node {
                    position_type: PositionType::Absolute,
                    bottom: Val::Px(0.0),
                    left: Val::Px(18.0),
                    right: Val::Px(18.0),
                    height: Val::Px(18.0),
                    ..default()
                },
            ),
            (
                MenuResizeEdge::TopLeft,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    top: Val::Px(0.0),
                    width: Val::Px(20.0),
                    height: Val::Px(20.0),
                    ..default()
                },
            ),
            (
                MenuResizeEdge::TopRight,
                Node {
                    position_type: PositionType::Absolute,
                    right: Val::Px(0.0),
                    top: Val::Px(0.0),
                    width: Val::Px(20.0),
                    height: Val::Px(20.0),
                    ..default()
                },
            ),
            (
                MenuResizeEdge::BottomLeft,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    bottom: Val::Px(0.0),
                    width: Val::Px(20.0),
                    height: Val::Px(20.0),
                    ..default()
                },
            ),
            (
                MenuResizeEdge::BottomRight,
                Node {
                    position_type: PositionType::Absolute,
                    right: Val::Px(0.0),
                    bottom: Val::Px(0.0),
                    width: Val::Px(20.0),
                    height: Val::Px(20.0),
                    ..default()
                },
            ),
        ] {
            panel.spawn((
                Button,
                MenuResizeHandle(edge),
                node,
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.0)),
            ));
        }

    });
}



/// A page's contents (the Overview is built with the window).
fn page_contents(page: &mut ChildSpawnerCommands, page_id: MenuPage) {
    use super::zones::Group;
    match page_id {
        MenuPage::Overview => {}
        MenuPage::Lighting => {
            page.spawn(menu_text("Lighting", 25.0, Color::srgb(0.94, 0.95, 0.97)));
            page.spawn(menu_text("Global renderer controls and ambience light settings.", 13.0, Color::srgb(0.64, 0.70, 0.79)));
            super::global::spawn(page);
            page.spawn(menu_text("Region-aware ambience", 18.0, Color::srgb(0.94, 0.95, 0.97)));
            page.spawn(menu_text(
                "These fields edit the target selected on Regions. Click a value, type the new number, and press Enter to apply. Esc cancels. Shift adjusts by x10.",
                12.0,
                Color::srgb(0.64, 0.70, 0.79),
            ));
            super::zones::spawn_groups(page, &[Group::Lighting, Group::Colors, Group::Rim, Group::PlayerLight]);
        }
        MenuPage::Regions => super::zones::spawn_regions(page),
        MenuPage::Atmosphere => {
            page.spawn(menu_text("Atmosphere", 25.0, Color::srgb(0.94, 0.95, 0.97)));
            page.spawn(menu_text("Fog and post-processing overrides for the target selected on Regions.", 13.0, Color::srgb(0.64, 0.70, 0.79)));
            page.spawn(menu_text("Click a value, type the new number, and press Enter to apply. Esc cancels.", 12.0, Color::srgb(0.64, 0.70, 0.79)));
            super::zones::spawn_groups(page, &[Group::Fog, Group::PostProcessing]);
        }
        MenuPage::Presets => super::presets_page::spawn(page),
    }
}

/// Startup: the window, hidden unless DSOR_PANEL is set (for screenshots).
fn spawn(mut commands: Commands) {
    spawn_menu(&mut commands, std::env::var("DSOR_PANEL").is_ok());
}

pub(super) struct WindowPlugin;

impl Plugin for WindowPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn).add_systems(Update, (
            (update_menu_navigation, update_menu_appearance).chain(),
            update_menu_window,
            scroll_menu_pages,
        ));
    }
}
