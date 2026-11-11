use raylib::prelude::*;
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

// Hitboxes del panel de ajustes
const PANEL_RECT: Rectangle = Rectangle { x: 25.0, y: 170.0, width: 310.0, height: 290.0 };
const CLOSE_BTN_RECT: Rectangle = Rectangle { x: 33.0, y: 178.0, width: 30.0, height: 30.0 };
const ROW1_RECT: Rectangle = Rectangle { x: 40.0, y: 225.0, width: 280.0, height: 60.0 };
const ROW2_RECT: Rectangle = Rectangle { x: 40.0, y: 305.0, width: 280.0, height: 60.0 };
const TOGGLE1_RECT: Rectangle = Rectangle { x: 240.0, y: 237.0, width: 70.0, height: 36.0 };
const TOGGLE2_RECT: Rectangle = Rectangle { x: 240.0, y: 317.0, width: 70.0, height: 36.0 };
const SETTINGS_BTN_RECT: Rectangle = Rectangle { x: 10.0, y: 490.0, width: 55.0, height: 55.0 };

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
    position: Vector2,
    velocity: f32,
    rotation: f32,
}

impl Bird {
    fn new() -> Self {
        Self {
            position: Vector2::new(SCREEN_WIDTH as f32 / 4.0, SCREEN_HEIGHT as f32 / 2.0),
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
        self.position = Vector2::new(SCREEN_WIDTH as f32 / 4.0, SCREEN_HEIGHT as f32 / 2.0);
        self.velocity = 0.0;
        self.rotation = 0.0;
    }
    fn get_collision_rect(&self) -> Rectangle {
        Rectangle::new(
            self.position.x - BIRD_HALF,
            self.position.y - BIRD_HALF,
            BIRD_SIZE,
            BIRD_SIZE,
        )
    }
    fn draw(&self, d: &mut RaylibDrawHandle) {
        let rec = Rectangle::new(self.position.x, self.position.y, BIRD_SIZE, BIRD_SIZE);
        let origin = Vector2::new(BIRD_HALF, BIRD_HALF);
        d.draw_rectangle_pro(rec, origin, self.rotation, Color::YELLOW);
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
    fn get_top_rect(&self) -> Rectangle {
        Rectangle::new(self.x, 0.0, PIPE_WIDTH as f32, self.gap_y - PIPE_GAP as f32 / 2.0)
    }
    fn get_bottom_rect(&self) -> Rectangle {
        let bottom_y = self.gap_y + PIPE_GAP as f32 / 2.0;
        Rectangle::new(
            self.x,
            bottom_y,
            PIPE_WIDTH as f32,
            (SCREEN_HEIGHT - GROUND_HEIGHT) as f32 - bottom_y,
        )
    }
    fn collides_with(&self, bird: &Bird) -> bool {
        let bird_rect = bird.get_collision_rect();
        bird_rect.check_collision_recs(self.get_top_rect())
            || bird_rect.check_collision_recs(self.get_bottom_rect())
    }
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
    ground_texture: RenderTexture2D,
    score_buf: String,
    best_buf: String,
    fps_buf: String,
    rot_buf: String,
}

impl Game {
    fn new(rl: &mut RaylibHandle, thread: &RaylibThread) -> Self {
        let high_score = Self::load_high_score();
        let (show_fps, show_rotation) = Self::load_settings();

        // ── Pre-renderizar el patrón del suelo en una textura (una sola vez) ──
        let mut ground_tex = rl
            .load_render_texture(thread, GROUND_TEX_WIDTH, GROUND_HEIGHT as u32)
            .expect("Failed to create ground render texture");
        {
            let mut d = rl.begin_texture_mode(thread, &mut ground_tex);
            d.clear_background(Color::BROWN);
            let mut x = 0;
            while x < GROUND_TEX_WIDTH as i32 {
                d.draw_rectangle(x, 0, 10, 5, Color::DARKBROWN);
                x += 20;
            }
        } // ← drop del RaylibTextureMode → end_texture_mode

        // ── Semilla para el RNG basada en tiempo ─────────────────────────────
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
            ground_texture: ground_tex,
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

    // ⚠ Ahora recibe `&mut self` para poder reutilizar los buffers de texto
    fn draw_game(&mut self, d: &mut RaylibDrawHandle) {
        d.clear_background(Color::SKYBLUE);

        for pipe in &self.pipes {
            d.draw_rectangle_rec(pipe.get_top_rect(), Color::GREEN);
            d.draw_rectangle_rec(pipe.get_bottom_rect(), Color::GREEN);
        }

        // ── Suelo cacheado: 1 quad en vez de ~20 rectángulos ─────────────────
        d.draw_texture_rec(
            self.ground_texture.texture,
            Rectangle::new(
                self.ground_scroll,
                0.0,
                SCREEN_WIDTH as f32,
                GROUND_HEIGHT as f32,
            ),
            Vector2::new(0.0, (SCREEN_HEIGHT - GROUND_HEIGHT) as f32),
            Color::WHITE,
        );

        self.bird.draw(d);

        // ── Marcador (buffers reutilizables, cero asignaciones por frame) ────
        self.score_buf.clear();
        let _ = write!(&mut self.score_buf, "Score: {}", self.score);
        d.draw_text(&self.score_buf, 10, 10, 20, Color::WHITE);

        self.best_buf.clear();
        let _ = write!(&mut self.best_buf, "Best: {}", self.high_score);
        d.draw_text(&self.best_buf, 10, 36, 16, Color::GOLD);

        match self.state {
            GameState::Ready => {
                d.draw_text(
                    "TAP or SPACE to start",
                    SCREEN_WIDTH / 2 - 110,
                    SCREEN_HEIGHT / 2 - 30,
                    18,
                    Color::WHITE,
                );
                d.draw_text(
                    "S or gear: settings",
                    SCREEN_WIDTH / 2 - 85,
                    SCREEN_HEIGHT / 2 + 10,
                    14,
                    Color::GRAY,
                );
                d.draw_text(
                    "ESC: exit game",
                    SCREEN_WIDTH / 2 - 55,
                    SCREEN_HEIGHT / 2 + 28,
                    14,
                    Color::GRAY,
                );
            }
            GameState::GameOver => {
                d.draw_text(
                    "GAME OVER",
                    SCREEN_WIDTH / 2 - 75,
                    SCREEN_HEIGHT / 2 - 80,
                    28,
                    Color::RED,
                );
                // Reutiliza los buffers ya poblados arriba
                d.draw_text(
                    &self.score_buf,
                    SCREEN_WIDTH / 2 - 45,
                    SCREEN_HEIGHT / 2 - 25,
                    22,
                    Color::WHITE,
                );
                d.draw_text(
                    &self.best_buf,
                    SCREEN_WIDTH / 2 - 40,
                    SCREEN_HEIGHT / 2 + 10,
                    22,
                    Color::GOLD,
                );
                d.draw_text(
                    "TAP or SPACE to restart",
                    SCREEN_WIDTH / 2 - 110,
                    SCREEN_HEIGHT / 2 + 55,
                    18,
                    Color::WHITE,
                );
                d.draw_text(
                    "ESC: exit game",
                    SCREEN_WIDTH / 2 - 55,
                    SCREEN_HEIGHT / 2 + 80,
                    14,
                    Color::GRAY,
                );
            }
            _ => {}
        }
    }

    fn draw_settings_button(&self, d: &mut RaylibDrawHandle) {
        d.draw_rectangle_rec(SETTINGS_BTN_RECT, Color::new(50, 50, 60, 230));
        d.draw_rectangle_lines_ex(SETTINGS_BTN_RECT, 2.0, Color::LIGHTGRAY);
        let cx = (SETTINGS_BTN_RECT.x + SETTINGS_BTN_RECT.width / 2.0) as i32;
        let cy = (SETTINGS_BTN_RECT.y + SETTINGS_BTN_RECT.height / 2.0) as i32;
        d.draw_circle(cx, cy, 16.0, Color::LIGHTGRAY);
        d.draw_circle(cx, cy, 8.0, Color::new(50, 50, 60, 255));
        d.draw_circle(cx, cy, 4.0, Color::LIGHTGRAY);
    }

    fn draw_settings_panel(&self, d: &mut RaylibDrawHandle) {
        d.draw_rectangle_rec(PANEL_RECT, Color::new(85, 85, 85, 250));
        d.draw_rectangle_lines_ex(PANEL_RECT, 2.0, Color::LIGHTGRAY);

        d.draw_text("SETTINGS", 130, 185, 24, Color::WHITE);

        // Botón X
        d.draw_rectangle_rec(CLOSE_BTN_RECT, Color::new(160, 50, 50, 255));
        d.draw_rectangle_lines_ex(CLOSE_BTN_RECT, 2.0, Color::LIGHTGRAY);
        let cx = CLOSE_BTN_RECT.x as i32;
        let cy = CLOSE_BTN_RECT.y as i32;
        let cs = CLOSE_BTN_RECT.width as i32;
        d.draw_line(cx + 9, cy + 9, cx + cs - 9, cy + cs - 9, Color::WHITE);
        d.draw_line(cx + cs - 9, cy + 9, cx + 9, cy + cs - 9, Color::WHITE);

        // Fila 1: FPS
        let sel1 = self.settings_selection == 0;
        let row1_bg = if sel1 {
            Color::new(115, 115, 115, 255)
        } else {
            Color::new(95, 95, 95, 255)
        };
        d.draw_rectangle_rec(ROW1_RECT, row1_bg);
        d.draw_text("Show FPS", 52, 243, 20, Color::WHITE);
        let t1_bg = if self.show_fps {
            Color::new(40, 160, 60, 255)
        } else {
            Color::new(140, 40, 40, 255)
        };
        d.draw_rectangle_rec(TOGGLE1_RECT, t1_bg);
        d.draw_rectangle_lines_ex(TOGGLE1_RECT, 1.5, Color::WHITE);
        let t1_text = if self.show_fps { "ON" } else { "OFF" };
        let t1_w = d.measure_text(t1_text, 18);
        d.draw_text(
            t1_text,
            (TOGGLE1_RECT.x as i32) + ((TOGGLE1_RECT.width as i32 - t1_w) / 2),
            (TOGGLE1_RECT.y as i32) + 9,
            18,
            Color::WHITE,
        );

        // Fila 2: Rotación
        let sel2 = self.settings_selection == 1;
        let row2_bg = if sel2 {
            Color::new(115, 115, 115, 255)
        } else {
            Color::new(95, 95, 95, 255)
        };
        d.draw_rectangle_rec(ROW2_RECT, row2_bg);
        d.draw_text("Show Rotation", 52, 323, 20, Color::WHITE);
        let t2_bg = if self.show_rotation {
            Color::new(40, 160, 60, 255)
        } else {
            Color::new(140, 40, 40, 255)
        };
        d.draw_rectangle_rec(TOGGLE2_RECT, t2_bg);
        d.draw_rectangle_lines_ex(TOGGLE2_RECT, 1.5, Color::WHITE);
        let t2_text = if self.show_rotation { "ON" } else { "OFF" };
        let t2_w = d.measure_text(t2_text, 18);
        d.draw_text(
            t2_text,
            (TOGGLE2_RECT.x as i32) + ((TOGGLE2_RECT.width as i32 - t2_w) / 2),
            (TOGGLE2_RECT.y as i32) + 9,
            18,
            Color::WHITE,
        );

        d.draw_text("UP/DOWN to select", 105, 392, 14, Color::LIGHTGRAY);
        d.draw_text("SPACE to toggle", 115, 410, 14, Color::LIGHTGRAY);
        d.draw_text("ESC or X to close", 105, 428, 14, Color::LIGHTGRAY);
    }

    fn draw(&mut self, d: &mut RaylibDrawHandle, fps: u32) {
        self.draw_game(d);

        if self.state != GameState::Settings {
            self.draw_settings_button(d);
        }

        if self.state == GameState::Settings {
            d.draw_rectangle(0, 0, SCREEN_WIDTH, SCREEN_HEIGHT, Color::new(0, 0, 0, 160));
            self.draw_settings_panel(d);
        }

        // ── Overlays (buffers reutilizables) ────────────────────────────────
        if self.show_fps {
            self.fps_buf.clear();
            let _ = write!(&mut self.fps_buf, "FPS: {}", fps);
            let w = d.measure_text(&self.fps_buf, 14);
            d.draw_text(&self.fps_buf, SCREEN_WIDTH - w - 8, 8, 14, Color::LIME);
        }
        if self.show_rotation {
            self.rot_buf.clear();
            let _ = write!(&mut self.rot_buf, "Rot: {:+.0}", self.bird.rotation);
            let w = d.measure_text(&self.rot_buf, 14);
            d.draw_text(&self.rot_buf, SCREEN_WIDTH - w - 8, 26, 14, Color::LIME);
        }
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────
fn is_flap_input(rl: &RaylibHandle) -> bool {
    rl.is_key_pressed(KeyboardKey::KEY_SPACE)
        || rl.is_mouse_button_pressed(MouseButton::MOUSE_BUTTON_LEFT)
}

fn point_in_rect(point: Vector2, rect: Rectangle) -> bool {
    point.x >= rect.x
        && point.x <= rect.x + rect.width
        && point.y >= rect.y
        && point.y <= rect.y + rect.height
}

// ── Main ────────────────────────────────────────────────────────────────────
fn main() {
    let (mut rl, thread) = raylib::init()
        .size(SCREEN_WIDTH, SCREEN_HEIGHT)
        .title("Flappy Bird - raylib-rs (optimized)")
        .build();

    rl.set_window_size(SCREEN_WIDTH, SCREEN_HEIGHT);
    rl.set_target_fps(60);

    let mut game = Game::new(&mut rl, &thread);

    while !rl.window_should_close() {
        let dt = rl.get_frame_time();

        // ── ESC contextual ──────────────────────────────────────────────────
        if rl.is_key_pressed(KeyboardKey::KEY_ESCAPE) {
            if game.state == GameState::Settings {
                game.close_settings();
            } else {
                break;
            }
        }

        // ── Apertura de ajustes (S o clic en engranaje) ─────────────────────
        if game.state != GameState::Settings {
            let s_pressed = rl.is_key_pressed(KeyboardKey::KEY_S);
            let click_on_gear = rl.is_mouse_button_pressed(MouseButton::MOUSE_BUTTON_LEFT)
                && point_in_rect(rl.get_mouse_position(), SETTINGS_BTN_RECT);
            if s_pressed || click_on_gear {
                game.open_settings();
            }
        }

        // ── Lógica por estado ───────────────────────────────────────────────
        match game.state {
            GameState::Ready => {
                if is_flap_input(&rl) {
                    game.reset();
                }
            }
            GameState::Playing => {
                if is_flap_input(&rl) {
                    game.bird.flap();
                }
                game.update(dt);
            }
            GameState::GameOver => {
                if is_flap_input(&rl) {
                    game.reset();
                }
            }
            GameState::Settings => {
                if rl.is_key_pressed(KeyboardKey::KEY_UP) {
                    game.settings_selection = game.settings_selection.saturating_sub(1);
                }
                if rl.is_key_pressed(KeyboardKey::KEY_DOWN) {
                    game.settings_selection = (game.settings_selection + 1).min(1);
                }
                if rl.is_key_pressed(KeyboardKey::KEY_SPACE)
                    || rl.is_key_pressed(KeyboardKey::KEY_ENTER)
                {
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
                if rl.is_mouse_button_pressed(MouseButton::MOUSE_BUTTON_LEFT) {
                    let pos = rl.get_mouse_position();
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

        let fps = rl.get_fps();
        let mut d = rl.begin_drawing(&thread);
        game.draw(&mut d, fps);
    }

    game.save_high_score();
    game.save_settings();
}
