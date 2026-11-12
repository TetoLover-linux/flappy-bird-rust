// ─────────────────────────────────────────────────────────────────────────────
// Flappy Bird — port de raylib-rs a macroquad (escritorio + Android)
//
// macroquad es "inspirado en raylib": la API es casi 1:1, pero con backend
// móvil nativo (miniquad → EGL/OpenGL ES sobre NativeActivity), que es lo que
// raylib-rs NO ofrece (soporta Windows/Linux/macOS/wasm, Android ❌).
//
// Entrada: mouse en escritorio y touch en Android (touches()), sin assets
// externos, sin crates de rand (LCG propio) y con buffers de texto
// reutilizables → cero asignaciones por frame.
// ─────────────────────────────────────────────────────────────────────────────
use macroquad::prelude::*;
use std::fmt::Write;
use std::fs;

// ── Resolución 9:16 compacta (360:640) ──────────────────────────────────────
const SCREEN_WIDTH: i32 = 360;
const SCREEN_HEIGHT: i32 = 640;
const GRAVITY: f32 = 1500.0;
const JUMP_FORCE: f32 = -420.0;
const PIPE_WIDTH: i32 = 50;
const PIPE_GAP: i32 = 130;
const PIPE_SPEED: f32 = 160.0;
const PIPE_SPAWN_INTERVAL: f32 = 1.6;
const GROUND_HEIGHT: i32 = 80;
const BIRD_SIZE: f32 = 24.0;
const BIRD_HALF: f32 = BIRD_SIZE / 2.0;
const HIGHSCORE_FILE: &str = "highscore.txt";
const SETTINGS_FILE: &str = "settings.txt";
const MAX_ROTATION_DEG: f32 = 50.0;
const ROTATION_SPEED: f32 = 200.0;
// Hitboxes del panel de ajustes (macroquad usa Rect { x, y, w, h })
const PANEL_RECT: Rect = Rect { x: 25.0, y: 170.0, w: 310.0, h: 290.0 };
const CLOSE_BTN_RECT: Rect = Rect { x: 33.0, y: 178.0, w: 30.0, h: 30.0 };
const ROW1_RECT: Rect = Rect { x: 40.0, y: 225.0, w: 280.0, h: 60.0 };
const ROW2_RECT: Rect = Rect { x: 40.0, y: 305.0, w: 280.0, h: 60.0 };
const TOGGLE1_RECT: Rect = Rect { x: 240.0, y: 237.0, w: 70.0, h: 36.0 };
const TOGGLE2_RECT: Rect = Rect { x: 240.0, y: 317.0, w: 70.0, h: 36.0 };
const SETTINGS_BTN_RECT: Rect = Rect { x: 10.0, y: 490.0, w: 55.0, h: 55.0 };
// Textura del suelo: SCREEN_WIDTH + 20 para permitir scroll sin costuras
const GROUND_TEX_WIDTH: u32 = (SCREEN_WIDTH + 20) as u32;

// ── SimpleRng: reemplaza la crate `rand` con un LCG de 2 líneas ─────────────
struct SimpleRng(u64);
impl SimpleRng {
    fn new(seed: u64) -> Self {
        Self(if seed == 0 { 1 } else { seed })
    }
    #[inline]
    fn next(&mut self) -> u64 {
        // LCG (constantes de Numerical Recipes)
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    #[inline]
    fn range_f32(&mut self, min: f32, max: f32) -> f32 {
        // 24 bits de entropía → suficiente precisión para f32
        let bits = (self.next() >> 40) as u32;
        let t = bits as f32 / (1u32 << 24) as f32; // [0, 1)
        min + t * (max - min)
    }
}

// ── Estados ─────────────────────────────────────────────────────────────────
#[derive(PartialEq, Clone, Copy)]
enum GameState {
    Ready,
    Playing,
    GameOver,
    Settings,
}

// ── Pájaro ──────────────────────────────────────────────────────────────────
struct Bird {
    position: Vec2,
    velocity: f32,
    rotation: f32,
}
impl Bird {
    fn new() -> Self {
        Self {
            position: vec2(SCREEN_WIDTH as f32 / 4.0, SCREEN_HEIGHT as f32 / 2.0),
            velocity: 0.0,
            rotation: 0.0,
        }
    }
    fn flap(&mut self) {
        self.velocity = JUMP_FORCE;
        self.rotation = -MAX_ROTATION_DEG;
    }
    fn update(&mut self, dt: f32) {
        self.velocity += GRAVITY * dt;
        self.position.y += self.velocity * dt;
        if self.velocity > 0.0 && self.rotation < MAX_ROTATION_DEG {
            self.rotation = (self.rotation + ROTATION_SPEED * dt).min(MAX_ROTATION_DEG);
        }
    }
    fn reset(&mut self) {
        self.position = vec2(SCREEN_WIDTH as f32 / 4.0, SCREEN_HEIGHT as f32 / 2.0);
        self.velocity = 0.0;
        self.rotation = 0.0;
    }
    fn get_collision_rect(&self) -> Rect {
        Rect::new(
            self.position.x - BIRD_HALF,
            self.position.y - BIRD_HALF,
            BIRD_SIZE,
            BIRD_SIZE,
        )
    }
    fn draw(&self) {
        // Equivalente a draw_rectangle_pro de raylib: rotación en radianes
        // alrededor del offset (centro del sprite).
        draw_rectangle_ex(
            self.position.x,
            self.position.y,
            BIRD_SIZE,
            BIRD_SIZE,
            DrawRectangleParams {
                offset: vec2(BIRD_HALF, BIRD_HALF),
                rotation: self.rotation.to_radians(),
                color: YELLOW,
            },
        );
    }
}

// ── Tubería ─────────────────────────────────────────────────────────────────
struct Pipe {
    x: f32,
    gap_y: f32,
    scored: bool,
}
impl Pipe {
    fn new(x: f32, gap_y: f32) -> Self {
        Self { x, gap_y, scored: false }
    }
    fn update(&mut self, dt: f32) -> bool {
        self.x -= PIPE_SPEED * dt;
        self.x + PIPE_WIDTH as f32 > 0.0
    }
    fn get_top_rect(&self) -> Rect {
        Rect::new(self.x, 0.0, PIPE_WIDTH as f32, self.gap_y - PIPE_GAP as f32 / 2.0)
    }
    fn get_bottom_rect(&self) -> Rect {
        let bottom_y = self.gap_y + PIPE_GAP as f32 / 2.0;
        Rect::new(
            self.x,
            bottom_y,
            PIPE_WIDTH as f32,
            (SCREEN_HEIGHT - GROUND_HEIGHT) as f32 - bottom_y,
        )
    }
    fn collides_with(&self, bird: &Bird) -> bool {
        // AABB clásico (equivalente a check_collision_recs de raylib)
        rects_collide(bird.get_collision_rect(), self.get_top_rect())
            || rects_collide(bird.get_collision_rect(), self.get_bottom_rect())
    }
}

#[inline]
fn rects_collide(a: Rect, b: Rect) -> bool {
    a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y
}

// Helpers: macroquad 0.4 no tiene draw_rectangle_rec(Rect, ..); envuelve las
// variantes por-coordenada para mantener el estilo de raylib.
#[inline]
fn draw_rect_rec(r: Rect, color: Color) {
    draw_rectangle(r.x, r.y, r.w, r.h, color);
}

#[inline]
fn draw_rect_lines(r: Rect, thickness: f32, color: Color) {
    draw_rectangle_lines(r.x, r.y, r.w, r.h, thickness, color);
}

#[inline]
fn point_in_rect(point: Vec2, rect: Rect) -> bool {
    point.x >= rect.x
        && point.x <= rect.x + rect.w
        && point.y >= rect.y
        && point.y <= rect.y + rect.h
}

// ── Juego ───────────────────────────────────────────────────────────────────
struct Game {
    state: GameState,
    prev_state: GameState,
    bird: Bird,
    pipes: Vec<Pipe>,
    score: u32,
    high_score: u32,
    spawn_timer: f32,
    ground_scroll: f32,
    show_fps: bool,
    show_rotation: bool,
    settings_selection: usize,
    // ── Optimizaciones ──────────────────────────────────────────────────────
    rng: SimpleRng,
    ground_texture: Texture2D,
    score_buf: String,
    best_buf: String,
    fps_buf: String,
    rot_buf: String,
}

/// Genera la textura del suelo UNA sola vez en CPU (un Image de 380x80 px) y
/// la sube a GPU: en el loop queda 1 quad con scroll por `source` rect,
/// en vez de ~20 draw_rectangle por frame.
fn build_ground_texture() -> Texture2D {
    let mut img = Image::gen_image_color(GROUND_TEX_WIDTH as u16, GROUND_HEIGHT as u16, BROWN);
    let w = GROUND_TEX_WIDTH as i32;
    let mut x = 0;
    while x < w {
        for dx in 0..10 {
            let px = (x + dx) as u32;
            if px < GROUND_TEX_WIDTH {
                for py in 0..5u32 {
                    img.set_pixel(px, py, DARKBROWN);
                }
            }
        }
        x += 20;
    }
    Texture2D::from_image(&img)
}

impl Game {
    fn new() -> Self {
        let high_score = Self::load_high_score();
        let (show_fps, show_rotation) = Self::load_settings();
        // ── Textura del suelo pre-renderizada (una sola vez) ────────────────
        let ground_texture = build_ground_texture();
        // ── Semilla para el RNG basada en tiempo ────────────────────────────
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0xDEAD_BEEF);
        Self {
            state: GameState::Ready,
            prev_state: GameState::Ready,
            bird: Bird::new(),
            pipes: Vec::with_capacity(8),
            score: 0,
            high_score,
            spawn_timer: 0.0,
            ground_scroll: 0.0,
            show_fps,
            show_rotation,
            settings_selection: 0,
            rng: SimpleRng::new(seed),
            ground_texture,
            score_buf: String::with_capacity(24),
            best_buf: String::with_capacity(24),
            fps_buf: String::with_capacity(16),
            rot_buf: String::with_capacity(16),
        }
    }
    fn load_high_score() -> u32 {
        fs::read_to_string(HIGHSCORE_FILE)
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
            .unwrap_or(0)
    }
    fn save_high_score(&self) {
        // En Android miniquad deja el cwd en el directorio privado de la app,
        // así que el archivo persiste en /data/data/<pkg>/files/.
        let _ = fs::write(HIGHSCORE_FILE, self.high_score.to_string());
    }
    fn load_settings() -> (bool, bool) {
        if let Ok(content) = fs::read_to_string(SETTINGS_FILE) {
            let mut lines = content.lines();
            let fps = lines
                .next()
                .and_then(|l| l.trim().parse::<u8>().ok())
                .unwrap_or(0)
                != 0;
            let rot = lines
                .next()
                .and_then(|l| l.trim().parse::<u8>().ok())
                .unwrap_or(0)
                != 0;
            (fps, rot)
        } else {
            (false, false)
        }
    }
    fn save_settings(&self) {
        let _ = fs::write(
            SETTINGS_FILE,
            format!("{}\n{}\n", self.show_fps as u8, self.show_rotation as u8),
        );
    }
    fn open_settings(&mut self) {
        self.prev_state = self.state;
        self.state = GameState::Settings;
        self.settings_selection = 0;
    }
    fn close_settings(&mut self) {
        self.state = self.prev_state;
        self.save_settings();
    }
    fn reset(&mut self) {
        self.state = GameState::Playing;
        self.bird.reset();
        self.pipes.clear();
        self.score = 0;
        self.spawn_timer = 0.0;
    }
    fn update(&mut self, dt: f32) {
        match self.state {
            GameState::Ready => {}
            GameState::Playing => {
                self.bird.update(dt);
                self.ground_scroll = (self.ground_scroll + PIPE_SPEED * dt) % 20.0;
                self.spawn_timer += dt;
                if self.spawn_timer >= PIPE_SPAWN_INTERVAL {
                    self.spawn_timer = 0.0;
                    let min_gap = 100.0;
                    let max_gap = (SCREEN_HEIGHT - GROUND_HEIGHT - 100) as f32;
                    let gap_y = self.rng.range_f32(min_gap, max_gap);
                    self.pipes.push(Pipe::new(SCREEN_WIDTH as f32, gap_y));
                }
                let mut score_increment = 0;
                self.pipes.retain_mut(|pipe| {
                    if !pipe.scored && pipe.x + (PIPE_WIDTH as f32) < self.bird.position.x {
                        pipe.scored = true;
                        score_increment += 1;
                    }
                    pipe.update(dt)
                });
                self.score += score_increment;
                if self.score > self.high_score {
                    self.high_score = self.score;
                }
                if self.bird.position.y + BIRD_HALF > (SCREEN_HEIGHT - GROUND_HEIGHT) as f32
                    || self.bird.position.y - BIRD_HALF < 0.0
                {
                    self.state = GameState::GameOver;
                    self.save_high_score();
                } else {
                    for pipe in &self.pipes {
                        if pipe.collides_with(&self.bird) {
                            self.state = GameState::GameOver;
                            self.save_high_score();
                            break;
                        }
                    }
                }
            }
            GameState::GameOver | GameState::Settings => {}
        }
    }
    // ── Marcador (buffers reutilizables, cero asignaciones por frame) ───────
    fn draw_game(&mut self) {
        clear_background(SKYBLUE);
        for pipe in &self.pipes {
            draw_rect_rec(pipe.get_top_rect(), GREEN);
            draw_rect_rec(pipe.get_bottom_rect(), GREEN);
        }
        // ── Suelo cacheado: 1 quad en vez de ~20 rectángulos ────────────────
        // Equivalente a draw_texture_rec: se dibuja solo el sub-rectángulo
        // [ground_scroll .. ground_scroll + 360] de la textura de 380 px.
        draw_texture_ex(
            &self.ground_texture,
            0.0,
            (SCREEN_HEIGHT - GROUND_HEIGHT) as f32,
            WHITE,
            DrawTextureParams {
                source: Some(Rect::new(
                    self.ground_scroll,
                    0.0,
                    SCREEN_WIDTH as f32,
                    GROUND_HEIGHT as f32,
                )),
                ..Default::default()
            },
        );
        self.bird.draw();
        self.score_buf.clear();
        let _ = write!(&mut self.score_buf, "Score: {}", self.score);
        draw_text(&self.score_buf, 10.0, 30.0, 20.0, WHITE);
        self.best_buf.clear();
        let _ = write!(&mut self.best_buf, "Best: {}", self.high_score);
        draw_text(&self.best_buf, 10.0, 56.0, 16.0, GOLD);
        match self.state {
            GameState::Ready => {
                draw_text(
                    "TAP or SPACE to start",
                    (SCREEN_WIDTH / 2 - 110) as f32,
                    (SCREEN_HEIGHT / 2 - 12) as f32,
                    18.0,
                    WHITE,
                );
                draw_text(
                    "S or gear: settings",
                    (SCREEN_WIDTH / 2 - 85) as f32,
                    (SCREEN_HEIGHT / 2 + 26) as f32,
                    14.0,
                    GRAY,
                );
                draw_text(
                    "ESC: exit game",
                    (SCREEN_WIDTH / 2 - 55) as f32,
                    (SCREEN_HEIGHT / 2 + 44) as f32,
                    14.0,
                    GRAY,
                );
            }
            GameState::GameOver => {
                draw_text(
                    "GAME OVER",
                    (SCREEN_WIDTH / 2 - 75) as f32,
                    (SCREEN_HEIGHT / 2 - 55) as f32,
                    28.0,
                    RED,
                );
                // Reutiliza los buffers ya poblados arriba
                draw_text(
                    &self.score_buf,
                    (SCREEN_WIDTH / 2 - 45) as f32,
                    (SCREEN_HEIGHT / 2) as f32,
                    22.0,
                    WHITE,
                );
                draw_text(
                    &self.best_buf,
                    (SCREEN_WIDTH / 2 - 40) as f32,
                    (SCREEN_HEIGHT / 2 + 35) as f32,
                    22.0,
                    GOLD,
                );
                draw_text(
                    "TAP or SPACE to restart",
                    (SCREEN_WIDTH / 2 - 110) as f32,
                    (SCREEN_HEIGHT / 2 + 80) as f32,
                    18.0,
                    WHITE,
                );
                draw_text(
                    "ESC: exit game",
                    (SCREEN_WIDTH / 2 - 55) as f32,
                    (SCREEN_HEIGHT / 2 + 105) as f32,
                    14.0,
                    GRAY,
                );
            }
            _ => {}
        }
    }
    fn draw_settings_button(&self) {
        draw_rect_rec(SETTINGS_BTN_RECT, Color::from_rgba(50, 50, 60, 230));
        draw_rect_lines(SETTINGS_BTN_RECT, 2.0, LIGHTGRAY);
        let cx = (SETTINGS_BTN_RECT.x + SETTINGS_BTN_RECT.w / 2.0) as f32;
        let cy = (SETTINGS_BTN_RECT.y + SETTINGS_BTN_RECT.h / 2.0) as f32;
        draw_circle(cx, cy, 16.0, LIGHTGRAY);
        draw_circle(cx, cy, 8.0, Color::from_rgba(50, 50, 60, 255));
        draw_circle(cx, cy, 4.0, LIGHTGRAY);
    }
    fn draw_settings_panel(&self) {
        draw_rect_rec(PANEL_RECT, Color::from_rgba(85, 85, 85, 250));
        draw_rect_lines(PANEL_RECT, 2.0, LIGHTGRAY);
        draw_text("SETTINGS", 130.0, 207.0, 24.0, WHITE);
        // Botón X
        draw_rect_rec(CLOSE_BTN_RECT, Color::from_rgba(160, 50, 50, 255));
        draw_rect_lines(CLOSE_BTN_RECT, 2.0, LIGHTGRAY);
        let cx = CLOSE_BTN_RECT.x as i32;
        let cy = CLOSE_BTN_RECT.y as i32;
        let cs = CLOSE_BTN_RECT.w as i32;
        draw_line(
            (cx + 9) as f32,
            (cy + 9) as f32,
            (cx + cs - 9) as f32,
            (cy + cs - 9) as f32,
            2.0,
            WHITE,
        );
        draw_line(
            (cx + cs - 9) as f32,
            (cy + 9) as f32,
            (cx + 9) as f32,
            (cy + cs - 9) as f32,
            2.0,
            WHITE,
        );
        // Fila 1: FPS
        let sel1 = self.settings_selection == 0;
        let row1_bg = if sel1 {
            Color::from_rgba(115, 115, 115, 255)
        } else {
            Color::from_rgba(95, 95, 95, 255)
        };
        draw_rect_rec(ROW1_RECT, row1_bg);
        draw_text("Show FPS", 52.0, 261.0, 20.0, WHITE);
        let t1_bg = if self.show_fps {
            Color::from_rgba(40, 160, 60, 255)
        } else {
            Color::from_rgba(140, 40, 40, 255)
        };
        draw_rect_rec(TOGGLE1_RECT, t1_bg);
        draw_rect_lines(TOGGLE1_RECT, 1.5, WHITE);
        let t1_text = if self.show_fps { "ON" } else { "OFF" };
        let t1_w = measure_text(t1_text, None, 18, 1.0).width;
        draw_text(
            t1_text,
            TOGGLE1_RECT.x + ((TOGGLE1_RECT.w - t1_w) / 2.0),
            TOGGLE1_RECT.y + 27.0,
            18.0,
            WHITE,
        );
        // Fila 2: Rotación
        let sel2 = self.settings_selection == 1;
        let row2_bg = if sel2 {
            Color::from_rgba(115, 115, 115, 255)
        } else {
            Color::from_rgba(95, 95, 95, 255)
        };
        draw_rect_rec(ROW2_RECT, row2_bg);
        draw_text("Show Rotation", 52.0, 341.0, 20.0, WHITE);
        let t2_bg = if self.show_rotation {
            Color::from_rgba(40, 160, 60, 255)
        } else {
            Color::from_rgba(140, 40, 40, 255)
        };
        draw_rect_rec(TOGGLE2_RECT, t2_bg);
        draw_rect_lines(TOGGLE2_RECT, 1.5, WHITE);
        let t2_text = if self.show_rotation { "ON" } else { "OFF" };
        let t2_w = measure_text(t2_text, None, 18, 1.0).width;
        draw_text(
            t2_text,
            TOGGLE2_RECT.x + ((TOGGLE2_RECT.w - t2_w) / 2.0),
            TOGGLE2_RECT.y + 27.0,
            18.0,
            WHITE,
        );
        draw_text("UP/DOWN to select", 105.0, 406.0, 14.0, LIGHTGRAY);
        draw_text("SPACE to toggle", 115.0, 424.0, 14.0, LIGHTGRAY);
        draw_text("ESC or X to close", 105.0, 442.0, 14.0, LIGHTGRAY);
    }
    fn draw(&mut self, fps: u32) {
        self.draw_game();
        if self.state != GameState::Settings {
            self.draw_settings_button();
        }
        if self.state == GameState::Settings {
            draw_rectangle(0.0, 0.0, SCREEN_WIDTH as f32, SCREEN_HEIGHT as f32, Color::from_rgba(0, 0, 0, 160));
            self.draw_settings_panel();
        }
        // ── Overlays (buffers reutilizables) ────────────────────────────────
        if self.show_fps {
            self.fps_buf.clear();
            let _ = write!(&mut self.fps_buf, "FPS: {}", fps);
            let w = measure_text(&self.fps_buf, None, 14, 1.0).width;
            draw_text(&self.fps_buf, SCREEN_WIDTH as f32 - w - 8.0, 22.0, 14.0, LIME);
        }
        if self.show_rotation {
            self.rot_buf.clear();
            let _ = write!(&mut self.rot_buf, "Rot: {:+.0}", self.bird.rotation);
            let w = measure_text(&self.rot_buf, None, 14, 1.0).width;
            draw_text(&self.rot_buf, SCREEN_WIDTH as f32 - w - 8.0, 40.0, 14.0, LIME);
        }
    }
}

// ── Entrada unificada mouse/touch ───────────────────────────────────────────
// macroquad reporta los toques por separado del mouse, así que se consultan
// ambas fuentes: en escritorio dispara el mouse, en Android el touch.
fn pointer_pressed() -> bool {
    is_mouse_button_pressed(MouseButton::Left)
        || touches().iter().any(|t| matches!(t.phase, TouchPhase::Started))
}

fn pointer_position() -> Vec2 {
    let ts = touches();
    if let Some(t) = ts.last() {
        t.position
    } else {
        let (x, y) = mouse_position();
        vec2(x, y)
    }
}

// ── Conf de ventana (escritorio; en Android es fullscreen ignorando el size) ─
fn window_conf() -> Conf {
    Conf {
        window_title: "Flappy Bird - macroquad (optimized)".to_owned(),
        window_width: SCREEN_WIDTH,
        window_height: SCREEN_HEIGHT,
        window_resizable: false,
        // vsync por defecto en miniquad = frame pacing a 60 Hz
        high_dpi: true,
        ..Default::default()
    }
}

// ── Main ────────────────────────────────────────────────────────────────────
// En escritorio genera fn main(); en Android exporta android_main (NativeActivity).
#[macroquad::main(window_conf)]
async fn main() {
    let mut game = Game::new();
    loop {
        if is_quit_requested() {
            break;
        }
        let dt = get_frame_time() as f32;
        // ── ESC contextual (botón "atrás" en Android) ───────────────────────
        if is_key_pressed(KeyCode::Escape) {
            if game.state == GameState::Settings {
                game.close_settings();
            } else {
                break;
            }
        }
        // ── Apertura de ajustes (S o toque en engranaje) ────────────────────
        if game.state != GameState::Settings {
            let s_pressed = is_key_pressed(KeyCode::S);
            let tap_on_gear = pointer_pressed() && point_in_rect(pointer_position(), SETTINGS_BTN_RECT);
            if s_pressed || tap_on_gear {
                game.open_settings();
            }
        }
        // ── Lógica por estado ───────────────────────────────────────────────
        match game.state {
            GameState::Ready => {
                if pointer_pressed() {
                    game.reset();
                }
            }
            GameState::Playing => {
                if pointer_pressed() {
                    game.bird.flap();
                }
                game.update(dt);
            }
            GameState::GameOver => {
                if pointer_pressed() {
                    game.reset();
                }
            }
            GameState::Settings => {
                if is_key_pressed(KeyCode::Up) {
                    game.settings_selection = game.settings_selection.saturating_sub(1);
                }
                if is_key_pressed(KeyCode::Down) {
                    game.settings_selection = (game.settings_selection + 1).min(1);
                }
                if is_key_pressed(KeyCode::Space) || is_key_pressed(KeyCode::Enter) {
                    match game.settings_selection {
                        0 => {
                            game.show_fps = !game.show_fps;
                            game.save_settings();
                        }
                        1 => {
                            game.show_rotation = !game.show_rotation;
                            game.save_settings();
                        }
                        _ => {}
                    }
                }
                if pointer_pressed() {
                    let pos = pointer_position();
                    if point_in_rect(pos, CLOSE_BTN_RECT) {
                        game.close_settings();
                    } else if point_in_rect(pos, TOGGLE1_RECT) {
                        game.show_fps = !game.show_fps;
                        game.settings_selection = 0;
                        game.save_settings();
                    } else if point_in_rect(pos, TOGGLE2_RECT) {
                        game.show_rotation = !game.show_rotation;
                        game.settings_selection = 1;
                        game.save_settings();
                    }
                }
            }
        }
        let fps = get_fps() as u32;
        game.draw(fps);
        next_frame().await;
    }
    game.save_high_score();
    game.save_settings();
}
