// game-module-visibility: pub
//! Client draw operations through the existing GUI drawing context.

use super::Game;
use horizon_client::{
    hud::{HudBatch, rgba},
    hud_layout::GuiDraw,
    text::Text,
};
use horizon_items::items::ItemStack as EngineStack;
use mod_api::{TextFieldLayout, TextFieldView};

impl Game<'_> {
    /// Paint GUI extraction calls through this window's atlas, including player faces.
    pub(super) fn native_gui_paint(&self, batch: &mut HudBatch, draws: &[GuiDraw]) {
        horizon_client::hud_layout::paint(batch, &self.gui.atlas, draws);
    }
    /// Vanilla's nine-slice widget rendering. `color` is ARGB, as `fill` uses.
    pub(super) fn native_gui_nine_slice(
        &self,
        batch: &mut HudBatch,
        sprite: &str,
        rect: [f32; 4],
        color: u32,
    ) {
        let [a, r, g, b] = color.to_be_bytes();
        horizon_client::ui::nine_slice(
            batch,
            &self.gui.atlas,
            sprite,
            rect[0],
            rect[1],
            rect[2],
            rect[3],
            rgba(r, g, b, a),
        );
    }
    /// A mod's button, drawn as the game's screens draw theirs (`ui::button`), or with the mod's
    /// own sprites (`ui::button_with_sprites`) when it has them.
    pub(super) fn native_widget_button(
        &self,
        batch: &mut HudBatch,
        [x, y, w, h]: [f32; 4],
        label: &Text,
        active: bool,
        highlighted: bool,
        sprites: Option<&mod_api::ButtonSprites>,
    ) {
        let Some(sprites) = sprites else {
            horizon_client::ui::button(
                batch,
                &self.gui.atlas,
                x,
                y,
                w,
                h,
                label,
                active,
                highlighted,
            );
            return;
        };
        horizon_client::ui::button_with_sprites(
            batch,
            &self.gui.atlas,
            [x, y, w, h],
            label,
            active,
            highlighted,
            false,
            &horizon_client::ui::ButtonSprites {
                normal: &sprites.normal,
                highlighted: sprites.highlighted.as_deref(),
                disabled: sprites.disabled.as_deref(),
                label_offset: sprites.label_offset,
            },
        );
    }
    /// A mod's text field, drawn as the game's screens draw theirs (`ui::EditBox`), from the
    /// state the mod keeps; answers the scroll it drew with and its character boundaries.
    pub(super) fn native_widget_text_field(
        &self,
        batch: &mut HudBatch,
        field: &TextFieldView,
    ) -> TextFieldLayout {
        let [x, y, w, h] = field.rect;
        let mut edit = horizon_client::ui::EditBox::new(x, y, w, h);
        edit.max_length = field.max_length;
        edit.value = field.value.clone();
        let len = horizon_client::ui::char_len(&edit.value);
        edit.cursor = field.cursor.min(len);
        edit.highlight = field.highlight.min(len);
        edit.set_scroll(field.scroll);
        edit.bordered = field.bordered;
        edit.focused = field.focused;
        let [a, r, g, b] = field.text_color.to_be_bytes();
        edit.text_color = rgba(r, g, b, a);
        edit.suggestion = field.suggestion.clone();
        edit.message = field.message.clone();
        edit.draw_with_caret(batch, &self.gui.atlas, None, field.caret_shown);
        let (scroll, edges) = edit.shown_edges(&self.gui.atlas);
        TextFieldLayout::new(scroll, edges)
    }
    /// A vanilla tooltip at the cursor, in GUI pixels.
    pub(super) fn native_gui_tooltip(&self, batch: &mut HudBatch, lines: &[Text], at: [f32; 2]) {
        horizon_client::ui::tooltip(batch, &self.gui.atlas, lines, (at[0], at[1]));
    }

    /// Draw an item stack with custom count/decoration text (or None) at `at` in GUI units.
    pub(super) fn native_gui_draw_item_with(
        &self,
        batch: &mut HudBatch,
        stack: &EngineStack,
        at: [f32; 2],
        text: Option<&str>,
    ) {
        self.draw_item_model(batch, stack, at[0], at[1]);
        self.draw_item_decorations(batch, stack, at[0], at[1], text.map(str::to_owned));
    }
    /// Whether a specific key is currently held down.
    /// `key` is the vanilla physical key name (`key.keyboard.left.shift`, for example).
    /// Unknown names are not down. Modifier combinations use `ClientHost::modifiers`.
    pub(super) fn is_key_down(&self, key: &str) -> bool {
        horizon_client::keybinds::key_code(key).is_some_and(|key| self.physical_keys.contains(&key))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "daily"]
    fn tooltip_helpers_read_live_keys_and_draw_count_overrides() {
        super::super::native_entity_refs::tests::with_client_game(|game| {
            use winit::keyboard::KeyCode;

            game.physical_keys.insert(KeyCode::ShiftLeft);
            assert!(game.is_key_down("key.keyboard.left.shift"));
            assert!(!game.is_key_down("key.keyboard.right.shift"));
            assert!(!game.is_key_down("not-a-key"));
            game.physical_keys.clear();
            assert!(!game.is_key_down("key.keyboard.left.shift"));

            let stack = EngineStack::new("minecraft:diamond", 5);
            let mut automatic = HudBatch::scratch([320, 240]);
            game.native_gui_draw_item_with(&mut automatic, &stack, [16.0, 32.0], None);
            let mut custom = HudBatch::scratch([320, 240]);
            game.native_gui_draw_item_with(&mut custom, &stack, [16.0, 32.0], Some("7"));
            assert!(!automatic.vertices.is_empty());
            assert_ne!(
                bytemuck::cast_slice::<_, u8>(&automatic.vertices),
                bytemuck::cast_slice::<_, u8>(&custom.vertices),
                "custom count text must reach the actual GUI geometry"
            );
        });
    }

    /// A mod's widgets draw with the game's own: a button as `ui::button` draws it (and with its
    /// own sprites as `ui::button_with_sprites` does), and a text field as `ui::EditBox` draws
    /// it, answering where each shown character ends so a press lands on it, and scrolling on to
    /// keep the cursor in view.
    #[test]
    #[ignore = "daily"]
    fn mod_widgets_draw_as_the_games_own() {
        use mod_api::client::widgets::{TextField, Widgets};
        super::super::native_entity_refs::tests::with_client_game(|game| {
            let label = Text::literal("Done");
            let mut ours = HudBatch::scratch([320, 240]);
            game.native_widget_button(
                &mut ours,
                [10.0, 10.0, 100.0, 20.0],
                &label,
                true,
                true,
                None,
            );
            let mut games = HudBatch::scratch([320, 240]);
            horizon_client::ui::button(
                &mut games,
                &game.gui.atlas,
                10.0,
                10.0,
                100.0,
                20.0,
                &label,
                true,
                true,
            );
            assert!(!ours.vertices.is_empty());
            assert_eq!(
                bytemuck::cast_slice::<_, u8>(&ours.vertices),
                bytemuck::cast_slice::<_, u8>(&games.vertices)
            );
            // The same button of the mod's own art: the game's sprite stays in the atlas, and
            // the mod's takes its place while keeping the label, the narration and the focus.
            let art = mod_api::ButtonSprites::new("gui/sprites/widget/button_disabled");
            let mut sprite = HudBatch::scratch([320, 240]);
            game.native_widget_button(
                &mut sprite,
                [10.0, 10.0, 100.0, 20.0],
                &label,
                true,
                true,
                Some(&art),
            );
            assert!(!sprite.vertices.is_empty());
            assert_ne!(
                bytemuck::cast_slice::<_, u8>(&sprite.vertices),
                bytemuck::cast_slice::<_, u8>(&ours.vertices),
                "the mod's own sprite is drawn instead of `widget/button`"
            );

            // The view a mod's group sends for a focused field, as `Widgets::draw` builds it.
            struct Draws<'a, 'g>(&'a super::Game<'g>, HudBatch, Vec<TextFieldView>);
            impl mod_api::DrawContext for Draws<'_, '_> {
                fn widget_text_field(&mut self, field: &TextFieldView) -> TextFieldLayout {
                    self.2.push(field.clone());
                    self.0.native_widget_text_field(&mut self.1, field)
                }
            }
            let mut widgets = Widgets::new();
            let id = widgets.add_text_field(TextField::new([10.0, 40.0, 60.0, 20.0], 64));
            let long = "a long name that does not fit in sixty pixels";
            widgets.text_field_mut(id).unwrap().set_value(long);
            widgets.set_focus(&*game, Some(id));
            let mut draws = Draws(game, HudBatch::scratch([320, 240]), Vec::new());
            widgets.draw(&mut draws, &*game, [0.0, 0.0]).unwrap();
            let view = draws.2[0].clone();
            assert!(!draws.1.vertices.is_empty());
            let layout = game.native_widget_text_field(&mut HudBatch::scratch([320, 240]), &view);
            // The cursor is at the end, so the field scrolled on to show it.
            assert!(layout.scroll > 0);
            let shown = layout.edges.len() - 1;
            assert_eq!(layout.scroll + shown, long.chars().count());
            assert_eq!(
                layout.edges[0], 14.0,
                "4 pixels in from a bordered field's left"
            );
            assert!(layout.edges.windows(2).all(|w| w[0] < w[1]));
            assert!(*layout.edges.last().unwrap() <= 10.0 + 60.0 - 4.0);
        });
    }

    /// #4491's acceptance probe: a screen of 50 buttons 14x15 px — the size
    /// `windowhutallinventory.xml:8` gives MineColonies' own art — each a three-character label
    /// and a sprite of its own, cut from the mod's own image so every button's art is a colour of
    /// its own. Every button shows its sprite, the highlighted one under the pointer, the disabled
    /// one when inactive; a click presses one and Tab and Enter reach the next; the screen draws
    /// in 16 ms a frame; and a pixel diff against the same screen drawn with the game's sprite
    /// changes the 50 rectangles and nothing else.
    #[test]
    #[ignore = "daily"]
    fn fifty_sprite_buttons_draw_their_own_art_and_still_answer_a_click() {
        use mod_api::client::widgets::{Button, ButtonSprites, Outcome, Widgets};
        use mod_platform::testing::with_mods;
        use std::time::{Duration, Instant};

        /// The mod's image is 1024 texels square: 20 blocks of 50 a row hold its 150 sprites.
        const BLOCK: u32 = 50;
        const ROW: u32 = 20;
        /// The HUD the probe draws.
        const SIZE: [u32; 2] = [320, 240];
        const IMAGE: u32 = horizon_client::hud::MOD_IMAGE_SIZE;
        let rect = |i: usize| {
            let (col, row) = (i % 10, i / 10);
            [8.0 + col as f32 * 18.0, 8.0 + row as f32 * 18.0, 14.0, 15.0]
        };
        let sprite = |i: usize, state: usize| format!("probe:gui/sprites/button_{i}_{state}");
        let colour = |i: usize, state: usize| {
            let n = (i * 3 + state) as u8;
            [
                n.wrapping_mul(37),
                n.wrapping_mul(53),
                n.wrapping_mul(29),
                255,
            ]
        };
        let block = |i: usize, state: usize| {
            let at = (i * 3 + state) as u32;
            ((at % ROW) * BLOCK, (at / ROW) * BLOCK)
        };
        // A batch's sprite quads, as `([x, y, x+w, y+h], [u0, v0, u1, v1])`: every quad of six
        // vertices that is not text, whatever the label's glyphs between them.
        let sprite_quads = |batch: &HudBatch| -> Vec<([f32; 4], [f32; 4])> {
            let sprite: Vec<_> = batch
                .vertices
                .iter()
                .filter(|v| v.text == 0)
                .copied()
                .collect();
            sprite
                .chunks(6)
                .map(|q| {
                    let (a, c) = (q[0], q[2]);
                    (
                        [a.pos[0], a.pos[1], c.pos[0], c.pos[1]],
                        [a.uv[0], a.uv[1], c.uv[0], c.uv[1]],
                    )
                })
                .collect()
        };
        let mut image = vec![0u8; (IMAGE * IMAGE * 4) as usize];
        for i in 0..50 {
            for state in 0..3 {
                let (bx, by) = block(i, state);
                for y in by..by + BLOCK {
                    for x in bx..bx + BLOCK {
                        let px = ((y * IMAGE + x) * 4) as usize;
                        image[px..px + 4].copy_from_slice(&colour(i, state));
                    }
                }
            }
        }

        /// A mod with no hook: it only uses the API, for the image slot on the atlas.
        struct ApiOnly;
        #[mod_api::__subscriptions]
        impl mod_api::Hooks for ApiOnly {}
        let mods = vec![mod_api::ModEntry::new("api-only", Box::new(ApiOnly))];

        /// A mod's group drawing through the game, as the screen forwarder does.
        struct Draws<'a, 'g>(&'a Game<'g>, HudBatch);
        impl mod_api::DrawContext for Draws<'_, '_> {
            fn widget_button(
                &mut self,
                rect: [f32; 4],
                label: &mod_api::Text,
                active: bool,
                highlighted: bool,
                sprites: Option<&ButtonSprites>,
            ) -> Result<(), mod_api::text::TextFormError> {
                let label = crate::game::api_values::TryIntoEngine::try_into_engine(label, &())?;
                self.0.native_widget_button(
                    &mut self.1,
                    rect,
                    &label,
                    active,
                    highlighted,
                    sprites,
                );
                Ok(())
            }
        }

        with_mods(mods.into(), || {
            super::super::native_entity_refs::tests::with_client_game(|game| {
                assert!(game.mod_image(IMAGE, IMAGE, &image));
                // One atlas slot for the whole image: each button's art is a block of it.
                let slot = game.gui.atlas.sprites[horizon_client::hud::MOD_IMAGE_SPRITE];
                let (du, dv) = (
                    (slot.u1 - slot.u0) / IMAGE as f32,
                    (slot.v1 - slot.v0) / IMAGE as f32,
                );
                for i in 0..50 {
                    for state in 0..3 {
                        let (bx, by) = block(i, state);
                        let (x, y) = (bx as f32 * du, by as f32 * dv);
                        game.gui.atlas.sprites.insert(
                            sprite(i, state),
                            horizon_client::hud::Rect {
                                u0: slot.u0 + x,
                                v0: slot.v0 + y,
                                u1: slot.u0 + x + BLOCK as f32 * du,
                                v1: slot.v0 + y + BLOCK as f32 * dv,
                                w: BLOCK,
                                h: BLOCK,
                            },
                        );
                    }
                }

                let mut widgets = Widgets::new();
                let ids: Vec<_> = (0..50)
                    .map(|i| {
                        widgets.add_button(
                            Button::new(rect(i), mod_api::Text::literal("Cut")).with_sprites(
                                ButtonSprites::new(sprite(i, 0))
                                    .with_highlighted(sprite(i, 1))
                                    .with_disabled(sprite(i, 2)),
                            ),
                        )
                    })
                    .collect();

                // Every button draws one quad: the atlas rect of its own sprite. The pointer is
                // over the second one, at [30, 12], which draws its highlighted sprite.
                let mut first = HudBatch::scratch(SIZE);
                {
                    let mut draws = Draws(game, first);
                    widgets.draw(&mut draws, &*game, [30.0, 12.0]).unwrap();
                    first = draws.1;
                }
                let quads = sprite_quads(&first);
                assert_eq!(quads.len(), 50, "one sprite a button, and no other quad");
                for (i, (dst, uv)) in quads.iter().enumerate() {
                    let art = &game.gui.atlas.sprites[&sprite(i, usize::from(i == 1))];
                    let [bx, by, bw, bh] = rect(i);
                    assert_eq!(
                        *dst,
                        [bx, by, bx + bw, by + bh],
                        "button {i}'s own rectangle"
                    );
                    assert_eq!(
                        *uv,
                        [art.u0, art.v0, art.u1, art.v1],
                        "button {i}'s own sprite"
                    );
                }

                // Inactive: the disabled sprite, and no press.
                widgets.button_mut(ids[4]).unwrap().active = false;
                let mut off = HudBatch::scratch(SIZE);
                {
                    let mut draws = Draws(game, off);
                    widgets.draw(&mut draws, &*game, [0.0, 0.0]).unwrap();
                    off = draws.1;
                }
                let art = &game.gui.atlas.sprites[&sprite(4, 2)];
                let [bx, by, ..] = rect(4);
                let quad = sprite_quads(&off)
                    .into_iter()
                    .find(|(dst, _)| (dst[0] - bx).abs() < 0.5 && (dst[1] - by).abs() < 0.5)
                    .expect("the inactive button's sprite");
                assert_eq!(
                    quad.1,
                    [art.u0, art.v0, art.u1, art.v1],
                    "an inactive button draws its disabled sprite"
                );
                let [bx, by, bw, bh] = rect(4);
                assert_eq!(
                    widgets.mouse_pressed(&*game, [bx + bw / 2.0, by + bh / 2.0], 0, 0),
                    Outcome::Ignored,
                    "a disabled button takes no press"
                );

                // A click presses one; Tab reaches the first and Enter presses it.
                assert_eq!(
                    widgets.mouse_pressed(&*game, [12.0, 12.0], 0, 0),
                    Outcome::Pressed(ids[0])
                );
                widgets.mouse_released(0);
                widgets.mouse_pressed(&*game, [300.0, 230.0], 0, 0);
                assert_eq!(
                    widgets.key_pressed(&*game, "key.keyboard.tab", 0),
                    Outcome::Handled
                );
                assert_eq!(widgets.focused(), Some(ids[0]));
                assert_eq!(
                    widgets.key_pressed(&*game, "key.keyboard.enter", 0),
                    Outcome::Pressed(ids[0])
                );
                // The frames below are a plain screen: no button has the keyboard focus.
                widgets.set_focus(&*game, None);

                // 50 sprite buttons in 16 ms a frame, on any machine that draws vanilla's.
                let mut frames = Duration::ZERO;
                for _ in 0..20 {
                    let start = Instant::now();
                    let mut draws = Draws(game, HudBatch::scratch(SIZE));
                    widgets.draw(&mut draws, &*game, [30.0, 12.0]).unwrap();
                    std::hint::black_box(draws.1.vertices.len());
                    frames += start.elapsed();
                }
                let per_frame = frames / 20;
                assert!(
                    per_frame < Duration::from_millis(16),
                    "{per_frame:?} a frame"
                );

                // A pixel diff with the same screen drawn with the game's sprite: the 50
                // rectangles change, whole, and nothing else does.
                let mut ours = HudBatch::scratch(SIZE);
                {
                    let mut draws = Draws(game, ours);
                    widgets.draw(&mut draws, &*game, [0.0, 0.0]).unwrap();
                    ours = draws.1;
                }
                let mut plain = widgets.clone();
                for id in &ids {
                    plain.button_mut(*id).unwrap().sprites = None;
                }
                let mut vanilla = HudBatch::scratch(SIZE);
                {
                    let mut draws = Draws(game, vanilla);
                    plain.draw(&mut draws, &*game, [0.0, 0.0]).unwrap();
                    vanilla = draws.1;
                }
                // Both frames upload the same patches — the mod's image and the label's glyphs —
                // so the only difference between them is the button art itself.
                let mut patches = std::mem::take(&mut ours.patches);
                patches.append(&mut game.map_patches);
                patches.append(&mut vanilla.patches);
                ours.patches = patches.clone();
                vanilla.patches = patches;
                let drawn = horizon_client::gfx::entity_pixel_test::render_hud(
                    &ours,
                    &game.gui.atlas,
                    SIZE,
                )
                .expect("software Vulkan draw");
                let games = horizon_client::gfx::entity_pixel_test::render_hud(
                    &vanilla,
                    &game.gui.atlas,
                    SIZE,
                )
                .expect("software Vulkan draw");
                let inside = |x: u32, y: u32| {
                    (0..50).any(|i| {
                        let [bx, by, bw, bh] = rect(i);
                        (bx..bx + bw).contains(&(x as f32)) && (by..by + bh).contains(&(y as f32))
                    })
                };
                let mut changed = [false; 50];
                for y in 0..SIZE[1] {
                    for x in 0..SIZE[0] {
                        let at = ((y * SIZE[0] + x) * 4) as usize;
                        let same = drawn[at..at + 4] == games[at..at + 4];
                        if !inside(x, y) {
                            assert!(same, "pixel ({x}, {y}) changed outside the 50 buttons");
                            continue;
                        }
                        for (i, flag) in changed.iter_mut().enumerate() {
                            let [bx, by, bw, bh] = rect(i);
                            if !same
                                && (bx..bx + bw).contains(&(x as f32))
                                && (by..by + bh).contains(&(y as f32))
                            {
                                *flag = true;
                            }
                        }
                    }
                }
                assert!(
                    changed.iter().all(|&c| c),
                    "every button's rectangle changed: {changed:?}"
                );
                // Each of them in its own colour, one pixel in from a corner the label never
                // reaches: the sprite that button names.
                for i in 0..50 {
                    let [bx, by, ..] = rect(i);
                    let at = (((by + 1.0) as u32 * SIZE[0] + (bx + 1.0) as u32) * 4) as usize;
                    let state = usize::from(i == 4) * 2;
                    assert_eq!(
                        &drawn[at..at + 3],
                        &colour(i, state)[..3],
                        "button {i}'s own art"
                    );
                }
            });
        });
    }
}
