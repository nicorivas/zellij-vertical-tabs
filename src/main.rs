use std::cmp::{max, min};
use std::collections::BTreeMap;
use unicode_width::UnicodeWidthStr;
use zellij_tile::prelude::*;

// ========== COLOR SYSTEM ==========

/// Color specification supporting default, 256-color, and RGB
#[derive(Debug, Clone, Copy, PartialEq, Default)]
enum ColorSpec {
    /// Use terminal default color
    #[default]
    Default,
    /// 256-color palette index (0-255)
    EightBit(u8),
    /// True color RGB
    Rgb(u8, u8, u8),
}

impl ColorSpec {
    /// Generate ANSI escape code for foreground color
    fn to_ansi_fg(self) -> String {
        match self {
            ColorSpec::Default => String::new(),
            ColorSpec::EightBit(n) => format!("\x1b[38;5;{}m", n),
            ColorSpec::Rgb(r, g, b) => format!("\x1b[38;2;{};{};{}m", r, g, b),
        }
    }

    /// Generate ANSI escape code for background color
    fn to_ansi_bg(self) -> String {
        match self {
            ColorSpec::Default => String::new(),
            ColorSpec::EightBit(n) => format!("\x1b[48;5;{}m", n),
            ColorSpec::Rgb(r, g, b) => format!("\x1b[48;2;{};{};{}m", r, g, b),
        }
    }

    fn is_default(self) -> bool {
        matches!(self, ColorSpec::Default)
    }
}

/// Parse a color value from string
/// Supports:
/// - Named colors: "accent", "dim", "red", etc.
/// - 256-color: "238"
/// - Hex RGB: "#444444" or "#444"
/// - RGB function: "rgb(68,68,68)"
fn parse_color_spec(name: &str) -> ColorSpec {
    let name = name.trim();

    // Check for RGB hex: #RGB or #RRGGBB
    if let Some(hex) = name.strip_prefix('#')
        && let Some((r, g, b)) = parse_hex_color(hex)
    {
        return ColorSpec::Rgb(r, g, b);
    }

    // Check for rgb(r,g,b) syntax
    if let Some(inner) = name.strip_prefix("rgb(").and_then(|s| s.strip_suffix(')'))
        && let Some((r, g, b)) = parse_rgb_func(inner)
    {
        return ColorSpec::Rgb(r, g, b);
    }

    // Check for numeric 256-color
    if let Ok(n) = name.parse::<u8>() {
        return ColorSpec::EightBit(n);
    }

    // Named colors mapped to 256-color approximations
    match name.to_lowercase().as_str() {
        // Default/reset
        "none" | "default" | "reset" => ColorSpec::Default,

        // Theme-like semantic colors (mapped to reasonable 256-color values)
        "accent" | "primary" => ColorSpec::EightBit(39), // Bright blue
        "secondary" => ColorSpec::EightBit(75),          // Light blue
        "tertiary" => ColorSpec::EightBit(141),          // Purple
        "muted" | "quaternary" => ColorSpec::EightBit(245), // Light gray
        "dim" | "dimmed" => ColorSpec::EightBit(240),    // Dark gray

        // Standard colors
        "black" => ColorSpec::EightBit(0),
        "red" | "error" | "warning" => ColorSpec::EightBit(196),
        "green" | "success" | "ok" => ColorSpec::EightBit(82),
        "yellow" => ColorSpec::EightBit(226),
        "blue" => ColorSpec::EightBit(33),
        "magenta" => ColorSpec::EightBit(201),
        "cyan" => ColorSpec::EightBit(51),
        "white" => ColorSpec::EightBit(15),
        "orange" => ColorSpec::EightBit(208),
        "gray" | "grey" => ColorSpec::EightBit(244),
        "pink" => ColorSpec::EightBit(213),
        "purple" => ColorSpec::EightBit(135),

        // Unknown - use default
        _ => ColorSpec::Default,
    }
}

/// Parse hex color: "444444" or "444" -> (r, g, b)
fn parse_hex_color(hex: &str) -> Option<(u8, u8, u8)> {
    match hex.len() {
        3 => {
            // #RGB -> expand to #RRGGBB
            let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 17;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 17;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 17;
            Some((r, g, b))
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some((r, g, b))
        }
        _ => None,
    }
}

/// Parse "r,g,b" -> (r, g, b)
fn parse_rgb_func(inner: &str) -> Option<(u8, u8, u8)> {
    let parts: Vec<&str> = inner.split(',').collect();
    if parts.len() != 3 {
        return None;
    }
    let r = parts[0].trim().parse::<u8>().ok()?;
    let g = parts[1].trim().parse::<u8>().ok()?;
    let b = parts[2].trim().parse::<u8>().ok()?;
    Some((r, g, b))
}

// ========== STYLE SYSTEM ==========

/// Inline style from #[...] directive
#[derive(Debug, Clone, Default)]
struct InlineStyle {
    fg: ColorSpec,
    bg: ColorSpec,
    bold: bool,
    dim: bool,
    fill: bool,
}

impl InlineStyle {
    /// Generate ANSI escape codes for this style (without reverse - that's handled at line level)
    fn to_ansi(&self) -> String {
        let mut result = String::new();

        // Attributes
        if self.bold {
            result.push_str("\x1b[1m");
        }
        if self.dim {
            result.push_str("\x1b[2m");
        }

        // Colors
        result.push_str(&self.fg.to_ansi_fg());
        result.push_str(&self.bg.to_ansi_bg());

        result
    }

    fn has_any_style(&self) -> bool {
        !self.fg.is_default() || !self.bg.is_default() || self.bold || self.dim || self.fill
    }
}

/// A segment of text with styling
#[derive(Debug, Clone)]
struct StyledSegment {
    text: String,
    style: InlineStyle,
}

impl StyledSegment {
    fn display_width(&self) -> usize {
        self.text.width()
    }
}

/// Collection of styled segments forming a complete styled string
#[derive(Debug, Clone, Default)]
struct StyledText {
    segments: Vec<StyledSegment>,
}

impl StyledText {
    fn new() -> Self {
        Self { segments: vec![] }
    }

    fn push(&mut self, text: String, style: InlineStyle) {
        if !text.is_empty() {
            self.segments.push(StyledSegment { text, style });
        }
    }

    fn display_width(&self) -> usize {
        self.segments.iter().map(|s| s.display_width()).sum()
    }

    /// Render to ANSI-coded string
    fn to_ansi(&self) -> String {
        let mut result = String::new();

        for segment in &self.segments {
            if segment.style.has_any_style() {
                result.push_str("\x1b[0m"); // Reset before applying new style
                result.push_str(&segment.style.to_ansi());
            }
            result.push_str(&segment.text);
        }

        // Reset at end
        if self.segments.iter().any(|s| s.style.has_any_style()) {
            result.push_str("\x1b[0m");
        }

        result
    }

    /// Truncate to fit within max_width display columns
    fn truncate(&self, max_width: usize) -> StyledText {
        if self.display_width() <= max_width {
            return self.clone();
        }

        let mut result = StyledText::new();
        let mut remaining = max_width;

        for segment in &self.segments {
            if remaining == 0 {
                break;
            }

            let seg_width = segment.display_width();
            if seg_width <= remaining {
                result.push(segment.text.clone(), segment.style.clone());
                remaining -= seg_width;
            } else {
                // Truncate this segment
                let mut truncated = String::new();
                let mut width = 0;
                for ch in segment.text.chars() {
                    let ch_width = ch.to_string().width();
                    if width + ch_width > remaining {
                        break;
                    }
                    truncated.push(ch);
                    width += ch_width;
                }
                result.push(truncated, segment.style.clone());
                break;
            }
        }

        result
    }
}

// ========== FORMAT PARSING ==========

/// Token from parsing a tmux-style format string
#[derive(Debug, Clone)]
enum FormatToken {
    /// Style directive: #[fg=color,bg=color,bold,dim]
    Style(InlineStyle),
    /// Variable with optional width: {var} or {=12:var}
    Variable { name: String, width: Option<usize> },
    /// Plain text
    Literal(String),
}

/// Parse a tmux-style format string into tokens
/// Supports: #[fg=color,bg=color,bold,dim], {variable}, {=width:variable}, #{variable}
fn parse_tmux_format(format: &str) -> Vec<FormatToken> {
    let mut tokens = Vec::new();
    let mut chars = format.chars().peekable();
    let mut literal = String::new();

    while let Some(ch) = chars.next() {
        if ch == '#' {
            match chars.peek() {
                Some('[') => {
                    // Flush literal
                    if !literal.is_empty() {
                        tokens.push(FormatToken::Literal(std::mem::take(&mut literal)));
                    }
                    chars.next(); // consume '['
                    // Parse style directive until ']'
                    let mut style_str = String::new();
                    while let Some(&c) = chars.peek() {
                        if c == ']' {
                            chars.next();
                            break;
                        }
                        style_str.push(chars.next().unwrap());
                    }
                    tokens.push(FormatToken::Style(parse_style_directive(&style_str)));
                }
                Some('{') => {
                    // Flush literal
                    if !literal.is_empty() {
                        tokens.push(FormatToken::Literal(std::mem::take(&mut literal)));
                    }
                    chars.next(); // consume '{'
                    let var_token = parse_variable(&mut chars);
                    tokens.push(var_token);
                }
                _ => {
                    literal.push(ch);
                }
            }
        } else if ch == '{' {
            // Flush literal
            if !literal.is_empty() {
                tokens.push(FormatToken::Literal(std::mem::take(&mut literal)));
            }
            let var_token = parse_variable(&mut chars);
            tokens.push(var_token);
        } else {
            literal.push(ch);
        }
    }

    if !literal.is_empty() {
        tokens.push(FormatToken::Literal(literal));
    }

    tokens
}

/// Parse style directive content: "fg=color,bg=color,bold,dim"
fn parse_style_directive(content: &str) -> InlineStyle {
    let mut style = InlineStyle::default();

    for part in content.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        if let Some(color_str) = part.strip_prefix("fg=") {
            style.fg = parse_color_spec(color_str);
        } else if let Some(color_str) = part.strip_prefix("bg=") {
            style.bg = parse_color_spec(color_str);
        } else if part == "bold" {
            style.bold = true;
        } else if part == "dim" {
            style.dim = true;
        } else if part == "fill" {
            style.fill = true;
        } else if part == "default" || part == "none" || part == "reset" {
            style = InlineStyle::default();
        }
    }

    style
}

/// Parse variable content after '{': "var}" or "=12:var}"
fn parse_variable(chars: &mut std::iter::Peekable<std::str::Chars>) -> FormatToken {
    let mut content = String::new();
    while let Some(&c) = chars.peek() {
        if c == '}' {
            chars.next();
            break;
        }
        content.push(chars.next().unwrap());
    }

    // Check for width specifier: =12:varname
    if let Some(rest) = content.strip_prefix('=')
        && let Some(colon_pos) = rest.find(':')
    {
        let width_str = &rest[..colon_pos];
        let var_name = &rest[colon_pos + 1..];
        if let Ok(width) = width_str.parse::<usize>() {
            return FormatToken::Variable {
                name: var_name.to_string(),
                width: Some(width),
            };
        }
    }

    FormatToken::Variable {
        name: content,
        width: None,
    }
}

/// Parse a styled string like "#[fg=240]│" into StyledText
fn parse_styled_string(s: &str) -> StyledText {
    let tokens = parse_tmux_format(s);
    let mut result = StyledText::new();
    let mut current_style = InlineStyle::default();

    for token in tokens {
        match token {
            FormatToken::Style(style) => {
                current_style = style;
            }
            FormatToken::Literal(text) => {
                result.push(text, current_style.clone());
            }
            FormatToken::Variable { name, .. } => {
                // Variables in border strings are not expanded, treat as literal
                result.push(format!("{{{}}}", name), current_style.clone());
            }
        }
    }

    result
}

// ========== CONFIGURATION ==========

/// Styling configuration for tab labels
#[derive(Clone)]
struct StyleConfig {
    format: String,
    format_active: String,
    overflow_above: String,
    overflow_below: String,
    indicator_active: String,
    indicator_fullscreen: String,
    indicator_sync: String,
    padding_top: usize,
    border: String,
    max_name_length: usize,
    start_index: usize,
    activity_format: String,
    estado_format: String,
    /// tabs que van ARRIBA, en su propia sección y sin número (p.ej. "hoy")
    arriba: Vec<String>,
    format_arriba: String,
    format_arriba_active: String,
}

impl Default for StyleConfig {
    fn default() -> Self {
        Self {
            format: "{num} {prio} {name}{atencion}".to_string(),
            // fill: la fila entera del tab activo con fondo (gris bajo, 237)
            format_active: "#[bg=237,fill]{num} {prio} {name}{indicators}{atencion}".to_string(),
            overflow_above: "  ^ +{count}".to_string(),
            overflow_below: "  v +{count}".to_string(),
            indicator_active: String::new(), // el fondo del tab activo basta
            indicator_fullscreen: "Z".to_string(),
            indicator_sync: "S".to_string(),
            max_name_length: 20,
            padding_top: 0,
            // dos columnas en negro al borde derecho: canal entre la barra y el contenido
            // (no las pinta el fondo del tab activo). Config `border` lo cambia.
            border: "  ".to_string(),
            start_index: 1,
            activity_format: "#[fg=dim]{activity}".to_string(),
            estado_format: "{estado}".to_string(),
            arriba: vec!["hoy".to_string()],
            format_arriba: "     {name}{atencion}".to_string(),
            format_arriba_active: "#[bg=237,fill]     {name}{indicators}{atencion}".to_string(),
        }
    }
}

// ========== ESTADO POR TAB (fork nicorivas: los tabs son proyectos) ==========
//
// Cada tab puede tener un estado que sobrevive a la sesión: una línea libre
// ("qué está pasando") y una lista de pendientes. La fuente de verdad es un JSON
// en el host (`estado_file` en la config del layout), con la forma
//   { "<nombre del tab>": { "estado": "...", "pendientes": ["...", "▶ en curso"] } }
// El plugin lo lee con `cat` al cargar y cada vez que recibe el pipe
// `estado_reload`. Un pendiente que empieza con "▶ " se dibuja como en curso.

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct Estado {
    #[serde(default)]
    estado: String,
    #[serde(default)]
    pendientes: Vec<String>,
}

const MAX_PENDIENTES: usize = 6;

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct AtencionEntrada {
    #[serde(default)]
    estado: String,
}

/// pipe `mudar`: mover los panes de terminal del tab `desde` al tab `hacia`.
/// Solo actúa la instancia que vive en `desde` (una vez, aunque haya N instancias).
#[derive(Debug, Clone, Default, serde::Deserialize)]
struct MudarPipe {
    #[serde(default)]
    desde: String,
    #[serde(default)]
    hacia: String,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct AtencionPipe {
    #[serde(default)]
    tab: String,
    #[serde(default)]
    estado: String,
}

// ========== PLUGIN STATE ==========

#[derive(Default)]
struct State {
    tabs: Vec<TabInfo>,
    active_tab_idx: usize,
    mode_info: ModeInfo,
    pane_manifest: PaneManifest,
    style: StyleConfig,
    last_rows: usize,
    permissions_granted: bool,
    is_selectable: bool,
    pending_events: Vec<Event>,
    activity: BTreeMap<String, activity::Activity>,
    own_session: String,
    estado: BTreeMap<String, Estado>,
    estado_file: String,
    /// semáforo de atención por tab: "trabajando" | "espera" | "listo" (hooks de Claude Code)
    atencion: BTreeMap<String, String>,
    atencion_file: String,
    /// bitácora de foco: cada cambio de tab activo, anotado por la instancia de ese tab
    tiempo_file: String,
    /// tabs archivados (archivados.json): Claude cerrado, tab listo para retomar. Ocultos salvo ⌥A.
    archivados: std::collections::BTreeSet<String>,
    archivados_file: String,
    mostrar_archivados: bool,
    /// filas de estado/pendientes bajo cada tab: apagadas por defecto (marca `filas.on`)
    filas_estado: bool,
    /// orden de la lista numerada: "" (Zellij), "alfa", "reciente", "prioridad" (archivo `orden`)
    orden: String,
    /// prioridad manual por tab (prioridades.json): 1 alta … 3 baja; sin prioridad = última
    prioridades: BTreeMap<String, u8>,
    /// último foco por tab (tail de tiempo.log): "AAAA-MM-DDTHH:MM:SS"
    ultimo_foco: BTreeMap<String, String>,
    /// hay un temporizador de sondeo programado (evita que se apilen)
    sondeo_programado: bool,
    /// última fila: columna donde empiezan ⌥A y ⌥O (para el clic) y cuántas filas hay
    col_arch: usize,
    col_orden: usize,
    ultimas_filas: usize,
    propio_id: u32,
    ultimo_tab_activo: String,
    /// fila dibujada -> índice (0-based) del tab al que pertenece
    row_map: Vec<Option<usize>>,
}

register_plugin!(State);

impl ZellijPlugin for State {
    fn load(&mut self, configuration: BTreeMap<String, String>) {
        // El hook de zellij-tile vuelve a tomar el mutex del estado al reportar un
        // pánico y se pierde el mensaje original; este lo escribe a stderr (va al
        // log de Zellij) antes de abortar.
        std::panic::set_hook(Box::new(|info| {
            eprintln!("PANIC zellij-vertical-tabs: {}", info);
        }));
        // Parse style configuration
        if let Some(v) = configuration.get("format") {
            self.style.format = v.clone();
        }
        if let Some(v) = configuration.get("format_active") {
            self.style.format_active = v.clone();
        }
        if let Some(v) = configuration.get("overflow_above") {
            self.style.overflow_above = v.clone();
        }
        if let Some(v) = configuration.get("overflow_below") {
            self.style.overflow_below = v.clone();
        }
        if let Some(v) = configuration.get("indicator_active") {
            self.style.indicator_active = v.clone();
        }
        if let Some(v) = configuration.get("indicator_fullscreen") {
            self.style.indicator_fullscreen = v.clone();
        }
        if let Some(v) = configuration.get("indicator_sync") {
            self.style.indicator_sync = v.clone();
        }
        if let Some(v) = configuration.get("max_name_length")
            && let Ok(n) = v.parse::<usize>()
        {
            self.style.max_name_length = n;
        }
        if let Some(v) = configuration.get("padding_top")
            && let Ok(n) = v.parse::<usize>()
        {
            self.style.padding_top = n;
        }
        if let Some(v) = configuration.get("border") {
            self.style.border = v.clone();
        } else if let Some(v) = configuration.get("border_char") {
            self.style.border = v.clone();
        }
        if let Some(v) = configuration.get("start_index")
            && let Ok(n) = v.parse::<usize>()
        {
            self.style.start_index = n;
        }
        if let Some(v) = configuration.get("activity_format") {
            self.style.activity_format = v.clone();
        }
        if let Some(v) = configuration.get("estado_format") {
            self.style.estado_format = v.clone();
        }
        if let Some(v) = configuration.get("arriba") {
            self.style.arriba = v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        }
        if let Some(v) = configuration.get("format_arriba") {
            self.style.format_arriba = v.clone();
        }
        if let Some(v) = configuration.get("format_arriba_active") {
            self.style.format_arriba_active = v.clone();
        }
        if let Some(v) = configuration.get("estado_file") {
            self.estado_file = v.clone();
        }
        if let Some(v) = configuration.get("atencion_file") {
            self.atencion_file = v.clone();
        }
        if let Some(v) = configuration.get("tiempo_file") {
            self.tiempo_file = v.clone();
        }
        // sin config explícita, viven junto a estado.json
        if let Some(dir) = std::path::Path::new(&self.estado_file).parent().map(|d| d.to_string_lossy().to_string())
            && !self.estado_file.is_empty()
        {
            if self.atencion_file.is_empty() {
                self.atencion_file = format!("{}/atencion.json", dir);
            }
            if self.tiempo_file.is_empty() {
                self.tiempo_file = format!("{}/tiempo.log", dir);
            }
            if self.archivados_file.is_empty() {
                self.archivados_file = format!("{}/archivados.json", dir);
            }
        }
        self.propio_id = get_plugin_ids().plugin_id;

        request_permission(&[
            PermissionType::ReadApplicationState,
            PermissionType::ChangeApplicationState,
            PermissionType::RunCommands,
            PermissionType::ReadCliPipes, // para unblock_cli_pipe_input
        ]);

        subscribe(&[
            EventType::TabUpdate,
            EventType::PaneUpdate,
            EventType::ModeUpdate,
            EventType::Mouse,
            EventType::PermissionRequestResult,
            EventType::SessionUpdate,
            EventType::RunCommandResult,
            EventType::Timer,
        ]);
    }

    fn update(&mut self, event: Event) -> bool {
        let mut should_render = false;

        if let Event::PermissionRequestResult(status) = event {
            if status == PermissionStatus::Granted {
                self.permissions_granted = true;
                self.is_selectable = false;
                set_selectable(false);

                while !self.pending_events.is_empty() {
                    let cached_event = self.pending_events.remove(0);
                    self.update(cached_event);
                }
                self.sondear();
                should_render = true;
            }
            return should_render;
        }

        if !self.permissions_granted {
            self.pending_events.push(event);
            return false;
        }

        match event {
            Event::PermissionRequestResult(_) => {}
            Event::ModeUpdate(mode_info) => {
                if self.mode_info != mode_info {
                    should_render = true;
                }
                self.mode_info = mode_info;
            }
            Event::TabUpdate(tabs) => {
                let active_tab_index = tabs.iter().position(|t| t.active).unwrap_or(0);
                let active_tab_idx = active_tab_index + 1;
                if self.active_tab_idx != active_tab_idx || self.tabs != tabs {
                    should_render = true;
                }
                self.active_tab_idx = active_tab_idx;
                self.tabs = tabs;
                if let Some(t) = self.tabs.iter().find(|t| t.active) {
                    let nombre = t.name.clone();
                    let pos = t.position;
                    if nombre != self.ultimo_tab_activo {
                        self.ultimo_tab_activo = nombre.clone();
                        // "terminó" se lee al entrar al tab: desaparece la marca
                        if self.atencion.get(&nombre).map(|s| s == "listo").unwrap_or(false) {
                            self.atencion.remove(&nombre);
                            should_render = true;
                        }
                        // solo la instancia que vive en el tab activo anota (una vez por cambio)
                        if self.propio_tab() == Some(pos) {
                            self.anotar_tiempo(&nombre);
                        }
                    }
                }
            }
            Event::PaneUpdate(pane_manifest) => {
                self.pane_manifest = pane_manifest;
                should_render = true;
            }
            Event::Mouse(me) => match me {
                Mouse::LeftClick(row, col) => {
                    let (row, col) = (row as usize, col as usize);
                    if self.ultimas_filas > 0 && row + 1 == self.ultimas_filas {
                        // última fila: los atajos son clicables (instantáneo, sin esperar al sondeo)
                        if col >= self.col_orden {
                            self.abrir_orden();
                        } else if col >= self.col_arch {
                            self.mostrar_archivados = !self.mostrar_archivados;
                            self.marcar_archivados(self.mostrar_archivados);
                            should_render = true;
                        }
                    } else if let Some(idx) = self.get_tab_at_row(row) {
                        switch_tab_to(idx as u32);
                    }
                }
                Mouse::ScrollUp(_) => {
                    let prev_tab = max(self.active_tab_idx.saturating_sub(1), 1);
                    switch_tab_to(prev_tab as u32);
                }
                Mouse::ScrollDown(_) => {
                    let next_tab = min(self.active_tab_idx + 1, self.tabs.len());
                    switch_tab_to(next_tab as u32);
                }
                _ => {}
            },
            Event::Timer(_) => {
                self.sondeo_programado = false;
                self.sondear();
            }
            Event::RunCommandResult(_code, stdout, _stderr, ctx) => {
                if ctx.get("flow").map(|s| s.as_str()) == Some("sondeo") {
                    // estado @@ atención @@ archivados @@ mostrar
                    let texto = String::from_utf8_lossy(&stdout).to_string();
                    let partes: Vec<&str> = texto.split("\n@@\n").collect();
                    if let Some(s) = partes.first()
                        && let Ok(m) = serde_json::from_str::<BTreeMap<String, Estado>>(s.trim())
                    {
                        let huella = |e: &BTreeMap<String, Estado>| e.iter().map(|(k, v)| format!("{}|{}|{}", k, v.estado, v.pendientes.join("|"))).collect::<Vec<_>>();
                        if huella(&m) != huella(&self.estado) {
                            self.estado = m;
                            should_render = true;
                        }
                    }
                    if let Some(s) = partes.get(1)
                        && let Ok(m) = serde_json::from_str::<BTreeMap<String, AtencionEntrada>>(s.trim())
                    {
                        let nuevo: BTreeMap<String, String> = m.into_iter().filter(|(_, e)| !e.estado.is_empty()).map(|(k, e)| (k, e.estado)).collect();
                        if nuevo != self.atencion { self.atencion = nuevo; should_render = true; }
                    }
                    if let Some(s) = partes.get(2)
                        && let Ok(v) = serde_json::from_str::<Vec<String>>(s.trim())
                    {
                        let nuevo: std::collections::BTreeSet<String> = v.into_iter().collect();
                        if nuevo != self.archivados { self.archivados = nuevo; should_render = true; }
                    }
                    let mostrar = partes.get(3).map(|s| s.trim() == "1").unwrap_or(false);
                    if mostrar != self.mostrar_archivados { self.mostrar_archivados = mostrar; should_render = true; }
                    let filas = partes.get(4).map(|s| s.trim() == "1").unwrap_or(false);
                    if filas != self.filas_estado { self.filas_estado = filas; should_render = true; }
                    let orden = partes.get(5).map(|s| s.trim().to_string()).unwrap_or_default();
                    if orden != self.orden { self.orden = orden; should_render = true; }
                    if let Some(s) = partes.get(6)
                        && let Ok(m) = serde_json::from_str::<BTreeMap<String, u8>>(s.trim())
                        && m != self.prioridades
                    {
                        self.prioridades = m; should_render = true;
                    }
                    if let Some(s) = partes.get(7) {
                        let mut uf: BTreeMap<String, String> = BTreeMap::new();
                        for l in s.lines() {
                            if let Some((h, n)) = l.split_once('\t') {
                                uf.insert(n.trim().to_string(), h.trim().to_string());
                            }
                        }
                        if uf != self.ultimo_foco { self.ultimo_foco = uf; should_render = true; }
                    }
                    if !self.sondeo_programado {
                        self.sondeo_programado = true;
                        set_timeout(6.0);
                    }
                }
                if ctx.get("flow").map(|s| s.as_str()) == Some("archivados") {
                    if let Ok(v) = serde_json::from_slice::<Vec<String>>(&stdout) {
                        self.archivados = v.into_iter().collect();
                        should_render = true;
                    }
                }
                if ctx.get("flow").map(|s| s.as_str()) == Some("atencion") {
                    if let Ok(m) = serde_json::from_slice::<BTreeMap<String, AtencionEntrada>>(&stdout) {
                        self.atencion = m.into_iter().filter(|(_, e)| !e.estado.is_empty()).map(|(k, e)| (k, e.estado)).collect();
                        should_render = true;
                    }
                }
                if ctx.get("flow").map(|s| s.as_str()) == Some("estado") {
                    match serde_json::from_slice::<BTreeMap<String, Estado>>(&stdout) {
                        Ok(m) => {
                            self.estado = m;
                            should_render = true;
                        }
                        Err(_) => {
                            // JSON roto o archivo ausente: conservar lo último bueno
                        }
                    }
                }
            }
            Event::SessionUpdate(sessions, _) => {
                if let Some(s) = sessions.iter().find(|s| s.is_current_session)
                    && self.own_session != s.name
                {
                    self.own_session = s.name.clone();
                    should_render = true;
                }
            }
            _ => {}
        }
        should_render
    }

    fn pipe(&mut self, pipe_message: PipeMessage) -> bool {
        // Un pipe lanzado desde la CLI queda bloqueado hasta que algún plugin lo
        // suelta; si no, `zellij pipe` no vuelve nunca (visto en 0.44 y 0.45).
        if let PipeSource::Cli(id) = &pipe_message.source {
            unblock_cli_pipe_input(id);
        }
        match pipe_message.name.as_str() {
            "set_selectable" => {
                match pipe_message.payload.as_deref() {
                    Some("true") => {
                        self.is_selectable = true;
                        set_selectable(true);
                    }
                    Some("false") => {
                        self.is_selectable = false;
                        set_selectable(false);
                    }
                    _ => {}
                }
                false
            }
            "toggle_selectable" => {
                self.is_selectable = !self.is_selectable;
                set_selectable(self.is_selectable);
                false
            }
            "estado_reload" => {
                self.leer_estado();
                false
            }
            "mudar" => {
                if let Some(payload) = pipe_message.payload.as_deref()
                    && let Ok(m) = serde_json::from_str::<MudarPipe>(payload)
                {
                    self.mudar(&m.desde, &m.hacia);
                }
                false
            }
            "archivados_reload" => {
                self.leer_archivados();
                false
            }
            "archivados_toggle" => {
                self.mostrar_archivados = !self.mostrar_archivados;
                true
            }
            "atencion" => {
                if let Some(payload) = pipe_message.payload.as_deref()
                    && let Ok(a) = serde_json::from_str::<AtencionPipe>(payload)
                    && !a.tab.is_empty()
                {
                    if a.estado.is_empty() {
                        self.atencion.remove(&a.tab);
                    } else {
                        self.atencion.insert(a.tab, a.estado);
                    }
                    true
                } else {
                    false
                }
            }
            "activity" => {
                if let Some(payload) = pipe_message.payload.as_deref()
                    && let Some((zsession, name, act)) = activity::parse_activity(payload)
                {
                    self.activity
                        .insert(format!("{}\u{1}{}", zsession, name), act);
                    return true;
                }
                false
            }
            _ => false,
        }
    }

    fn render(&mut self, rows: usize, cols: usize) {
        self.last_rows = rows;

        if !self.permissions_granted || self.tabs.is_empty() {
            return;
        }

        self.render_vertical(rows, cols);
    }
}

impl State {
    /// Pide al host el archivo de estado; la respuesta llega como RunCommandResult.
    fn leer_estado(&mut self) {
        if self.estado_file.is_empty() {
            return;
        }
        // Un pipe puede llegar antes de que el host conceda los permisos (las
        // instancias cargan escalonadas). run_command sin permiso hace fallar la
        // llamada wasm y deja el mutex del estado tomado: la instancia queda
        // muerta. Al concederse los permisos se lee igual, así que basta con no llamar.
        if !self.permissions_granted {
            return;
        }
        let mut ctx = BTreeMap::new();
        ctx.insert("flow".to_string(), "estado".to_string());
        run_command(&["cat", &self.estado_file], ctx);
    }

    /// Sondeo: un solo `sh -c` lee estado.json, atencion.json, archivados.json y la marca
    /// de "mostrar archivados". Va por Timer/RunCommandResult, el camino serializado del
    /// host; los pipes NO lo son (reentran la instancia y revientan el mutex de stdout).
    fn sondear(&self) {
        if !self.permissions_granted || self.estado_file.is_empty() {
            return;
        }
        let dir = std::path::Path::new(&self.estado_file).parent().map(|d| d.to_string_lossy().to_string()).unwrap_or_default();
        let cmd = format!(
            "cat '{}' 2>/dev/null; printf '\\n@@\\n'; cat '{}' 2>/dev/null; printf '\\n@@\\n'; cat '{}' 2>/dev/null; printf '\\n@@\\n'; [ -e '{}/archivados.mostrar' ] && echo 1; printf '\\n@@\\n'; [ -e '{}/filas.on' ] && echo 1; printf '\\n@@\\n'; cat '{}/orden' 2>/dev/null; printf '\\n@@\\n'; cat '{}/prioridades.json' 2>/dev/null; printf '\\n@@\\n'; tail -n 400 '{}' 2>/dev/null",
            self.estado_file, self.atencion_file, self.archivados_file, dir, dir, dir, dir, self.tiempo_file
        );
        let mut ctx = BTreeMap::new();
        ctx.insert("flow".to_string(), "sondeo".to_string());
        run_command(&["sh", "-c", &cmd], ctx);
    }

    fn leer_archivados(&self) {
        if !self.permissions_granted || self.archivados_file.is_empty() {
            return;
        }
        let mut ctx = BTreeMap::new();
        ctx.insert("flow".to_string(), "archivados".to_string());
        run_command(&["cat", &self.archivados_file], ctx);
    }

    /// Mueve los panes de terminal del tab `desde` al tab `hacia`, si esta instancia
    /// vive en `desde`. El tab de origen, vacío, lo cierra Zellij.
    fn mudar(&self, desde: &str, hacia: &str) {
        let propio = match self.propio_tab() {
            Some(p) => p,
            None => return,
        };
        // Actúa la instancia que vive en el DESTINO (una sola, y viva aunque la del
        // origen haya muerto); el manifest trae los panes de todos los tabs.
        let destino = match self.tabs.iter().find(|t| t.name == hacia) {
            Some(t) => t.position,
            None => return,
        };
        if destino != propio {
            return;
        }
        let origen = match self.tabs.iter().find(|t| t.name == desde) {
            Some(t) => t.position,
            None => return,
        };
        let ids: Vec<PaneId> = self
            .pane_manifest
            .panes
            .get(&origen)
            .map(|ps| ps.iter().filter(|p| !p.is_plugin).map(|p| PaneId::Terminal(p.id)).collect())
            .unwrap_or_default();
        if !ids.is_empty() {
            break_panes_to_tab_with_index(&ids, destino, true);
        }
    }

    /// Escribe/borra la marca `archivados.mostrar` para que las demás instancias sigan al sondeo.
    fn marcar_archivados(&self, mostrar: bool) {
        if !self.permissions_granted || self.estado_file.is_empty() {
            return;
        }
        let dir = std::path::Path::new(&self.estado_file).parent().map(|d| d.to_string_lossy().to_string()).unwrap_or_default();
        let cmd = if mostrar { format!("touch '{}/archivados.mostrar'", dir) } else { format!("rm -f '{}/archivados.mostrar'", dir) };
        run_command(&["sh", "-c", &cmd], BTreeMap::new());
    }

    /// Abre el menú de orden (bin/flow-orden) como pane flotante.
    fn abrir_orden(&self) {
        if !self.permissions_granted || self.estado_file.is_empty() {
            return;
        }
        let dir = std::path::Path::new(&self.estado_file).parent().map(|d| d.to_string_lossy().to_string()).unwrap_or_default();
        let cmd = format!("zellij action new-pane --floating --close-on-exit -n '⌥O orden' -- '{}/bin/flow-orden'", dir);
        run_command(&["sh", "-c", &cmd], BTreeMap::new());
    }

    /// Posición del tab donde vive ESTA instancia (por su id de plugin en el manifest).
    fn propio_tab(&self) -> Option<usize> {
        self.pane_manifest
            .panes
            .iter()
            .find(|(_, panes)| panes.iter().any(|p| p.is_plugin && p.id == self.propio_id))
            .map(|(pos, _)| *pos)
    }

    /// Anota "hora<TAB>nombre" en tiempo.log (la hora la pone el host).
    fn anotar_tiempo(&self, nombre: &str) {
        if !self.permissions_granted || self.tiempo_file.is_empty() {
            return;
        }
        let seguro = nombre.replace('\'', "'\"'\"'");
        // Si junto al archivo hay bin/flow-foco, él anota (y hace lo que quiera con el
        // foco, p. ej. decir el estado del proyecto); si no, anotar aquí.
        let dir = std::path::Path::new(&self.tiempo_file).parent().map(|d| d.to_string_lossy().to_string()).unwrap_or_default();
        // flow-foco recibe también la posición (1-based) del tab: renombrar/abrir la
        // ficha por posición, no por el foco del cliente, que puede haberse movido.
        let pos1 = self.propio_tab().map(|p| p + 1).unwrap_or(0);
        let cmd = format!(
            "F='{}/bin/flow-foco'; if [ -x \"$F\" ]; then \"$F\" '{}' {}; else printf '%s\t%s\n' \"$(date +%Y-%m-%dT%H:%M:%S)\" '{}' >> '{}'; fi",
            dir, seguro, pos1, seguro, self.tiempo_file
        );
        let mut ctx = BTreeMap::new();
        ctx.insert("flow".to_string(), "tiempo".to_string());
        run_command(&["sh", "-c", &cmd], ctx);
    }

    /// Símbolo y color del semáforo de un tab.
    fn atencion_de(&self, tab: &str) -> (&'static str, ColorSpec) {
        match self.atencion.get(tab).map(|s| s.as_str()) {
            Some("trabajando") => ("●", ColorSpec::EightBit(4)),
            Some("espera") => ("○", ColorSpec::EightBit(3)),
            Some("listo") => ("✓", ColorSpec::EightBit(2)),
            _ => ("", ColorSpec::Default),
        }
    }

    /// Filas extra bajo un tab: su estado (archivo) y su actividad viva (pipe).
    /// Ya vienen con el formato de estilo aplicado; falta parsearlas y truncarlas.
    fn filas_extra(&self, tab: &TabInfo, cols: usize) -> Vec<String> {
        let mut extra = Vec::new();
        if !self.filas_estado {
            return extra; // apagadas por defecto: la ficha (⌥R) ya muestra el estado
        }
        if let Some(e) = self.estado.get(&tab.name) {
            if !e.estado.is_empty() {
                let fila = format!("  › {}", e.estado);
                extra.push(self.style.estado_format.replace("{estado}", &fila));
            }
            for p in e.pendientes.iter().take(MAX_PENDIENTES) {
                let (caja, texto) = match p.strip_prefix("▶ ") {
                    Some(t) => ("▣", t),
                    None => ("☐", p.as_str()),
                };
                let fila = format!("  {} {}", caja, texto);
                extra.push(self.style.activity_format.replace("{activity}", &fila));
            }
            if e.pendientes.len() > MAX_PENDIENTES {
                extra.push(self.style.activity_format.replace("{activity}", "  …"));
            }
        }
        // actividad viva: primero por nombre de tab (este fork), luego por título
        // del pane enfocado (comportamiento upstream, para productores como Claude Code)
        let por_tab = format!("{}\u{1}{}", self.own_session, norm_session_name(&tab.name));
        let act = self.activity.get(&por_tab).or_else(|| {
            let pane = self
                .get_focused_pane_title(tab.position)
                .map(|t| norm_session_name(&t))
                .unwrap_or_else(|| norm_session_name(&tab.name));
            self.activity.get(&format!("{}\u{1}{}", self.own_session, pane))
        });
        if let Some(act) = act {
            for arow in activity::render_activity(act, cols) {
                extra.push(self.style.activity_format.replace("{activity}", &arow));
            }
        }
        extra
    }

    fn get_focused_pane_title(&self, tab_position: usize) -> Option<String> {
        if let Some(panes) = self.pane_manifest.panes.get(&tab_position) {
            for pane in panes {
                if pane.is_focused && !pane.is_plugin {
                    let title = &pane.title;
                    if title.starts_with("Pane #") || title.starts_with("Tab #") || title.is_empty()
                    {
                        return None;
                    }
                    return Some(title.clone());
                }
            }
        }
        None
    }

    fn expand_overflow_format(&self, format: &str, count: usize) -> String {
        format.replace("{count}", &count.to_string())
    }

    /// Expand a tmux-style format string with tab info, returning styled text
    fn expand_tmux_format(&self, format: &str, tab: &TabInfo, index: usize) -> StyledText {
        let tokens = parse_tmux_format(format);
        let mut result = StyledText::new();
        let mut current_style = InlineStyle::default();

        // Get focused pane title for this tab
        let pane_title = self
            .get_focused_pane_title(tab.position)
            .or_else(|| {
                if !tab.name.starts_with("Tab #") {
                    Some(tab.name.clone())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "...".to_string());

        // Build indicators string
        let mut indicators = String::new();
        if tab.is_fullscreen_active {
            indicators.push_str(&self.style.indicator_fullscreen);
        }
        if tab.is_sync_panes_active {
            indicators.push_str(&self.style.indicator_sync);
        }
        if tab.active {
            indicators.push_str(&self.style.indicator_active);
        }

        for token in tokens {
            match token {
                FormatToken::Style(style) => {
                    current_style = style;
                }
                FormatToken::Variable { name, width } => {
                    if name == "num" {
                        result.push(format!("{:>2}", index), current_style.clone());
                        continue;
                    }
                    if name == "prio" {
                        // círculos, como el semáforo, pero en gris y en su propia columna:
                        // ● alta · ◐ media · ○ baja · nada sin prioridad
                        let (simbolo, color) = match self.prioridades.get(&tab.name) {
                            Some(1) => ("●", ColorSpec::EightBit(252)),
                            Some(2) => ("◐", ColorSpec::EightBit(246)),
                            Some(3) => ("○", ColorSpec::EightBit(240)),
                            _ => (" ", ColorSpec::Default),
                        };
                        let mut st = current_style.clone();
                        st.fg = color;
                        result.push(simbolo.to_string(), st);
                        continue;
                    }
                    if name == "atencion" || name == "a" {
                        let (texto, color) = self.atencion_de(&tab.name);
                        if !texto.is_empty() {
                            let mut st = current_style.clone();
                            st.fg = color;
                            st.dim = false;
                            result.push(format!(" {}", texto), st);
                        }
                        continue;
                    }
                    let value = match name.as_str() {
                        "index" | "i" => index.to_string(),
                        "name" | "n" => {
                            if tab.active
                                && self.mode_info.mode == InputMode::RenameTab
                                && tab.name.is_empty()
                            {
                                "Enter name...".to_string()
                            } else if !tab.name.starts_with("Tab #") && !tab.name.is_empty() {
                                tab.name.clone()
                            } else {
                                pane_title.clone()
                            }
                        }
                        "title" | "t" | "pane_title" => pane_title.clone(),
                        "indicators" => indicators.clone(),
                        "fullscreen" => {
                            if tab.is_fullscreen_active {
                                self.style.indicator_fullscreen.clone()
                            } else {
                                String::new()
                            }
                        }
                        "sync" => {
                            if tab.is_sync_panes_active {
                                self.style.indicator_sync.clone()
                            } else {
                                String::new()
                            }
                        }
                        "active" => {
                            if tab.active {
                                self.style.indicator_active.clone()
                            } else {
                                String::new()
                            }
                        }
                        _ => format!("{{{}}}", name),
                    };

                    let text = if let Some(w) = width {
                        truncate_string(&value, w)
                    } else {
                        truncate_string(&value, self.style.max_name_length)
                    };

                    result.push(text, current_style.clone());
                }
                FormatToken::Literal(text) => {
                    result.push(text, current_style.clone());
                }
            }
        }

        result
    }

    /// Build a complete line with content, padding, and border
    fn build_line(&self, content: &StyledText, cols: usize, is_selected: bool) -> String {
        let border = parse_styled_string(&self.style.border);
        let border_width = border.display_width();

        let effective_cols = cols.saturating_sub(border_width);

        // Truncate content if it exceeds available width to prevent wrapping
        let content = content.truncate(effective_cols);
        let content_width = content.display_width();
        let padding_needed = effective_cols.saturating_sub(content_width);

        let mut line = String::new();

        // Check if any segment has fill attribute - fills entire row with bg color
        let has_fill = is_selected && content.segments.iter().any(|s| s.style.fill);

        if has_fill {
            // Fill mode: use reverse video with swapped colors so bg fills the row
            // User writes #[bg=236,fill] -> we swap to fg=236 -> reverse makes displayed bg=236
            line.push_str("\x1b[7m");

            for segment in &content.segments {
                // Swap fg and bg for reverse video
                let mut swapped_style = segment.style.clone();
                std::mem::swap(&mut swapped_style.fg, &mut swapped_style.bg);
                swapped_style.fill = false; // Don't need fill flag in output

                if swapped_style.has_any_style() {
                    line.push_str("\x1b[0m\x1b[7m"); // Reset and re-apply reverse
                    line.push_str(&swapped_style.to_ansi());
                }
                line.push_str(&segment.text);
            }

            if padding_needed > 0 {
                line.push_str(&" ".repeat(padding_needed));
            }

            line.push_str("\x1b[0m");
        } else {
            // Normal rendering - bg colors only apply to text, not padding
            line.push_str(&content.to_ansi());

            if padding_needed > 0 {
                line.push_str(&" ".repeat(padding_needed));
            }
        }

        // Add border (not affected by selection)
        if border_width > 0 {
            line.push_str(&border.to_ansi());
        }

        line
    }

    /// Build a line with just the border (for empty rows)
    fn build_empty_line(&self, cols: usize) -> String {
        let border = parse_styled_string(&self.style.border);
        let border_width = border.display_width();

        if border_width == 0 {
            return " ".repeat(cols);
        }

        let effective_cols = cols.saturating_sub(border_width);
        let mut line = " ".repeat(effective_cols);
        line.push_str(&border.to_ansi());
        line
    }

    fn render_vertical(&mut self, rows: usize, cols: usize) {
        let top_padding = self.style.padding_top;

        // Sección de arriba: los tabs fijos (config `arriba`), sin número; luego
        // una línea en blanco y la lista numerada del resto.
        // Arriba van los tabs de `arriba` (hoy) y, colgando de ellos, los tabs de
        // reunión (◷), sin número y en otro color: no son proyectos.
        let mut fijos: Vec<usize> = self
            .style
            .arriba
            .iter()
            .filter_map(|n| self.tabs.iter().position(|t| &t.name == n))
            .collect();
        for i in 0..self.tabs.len() {
            if es_reunion(&self.tabs[i].name) && !fijos.contains(&i) {
                fijos.push(i);
            }
        }
        // Archivados: fuera de la lista numerada; van abajo, en su sección, solo si
        // ⌥A los muestra (o si el activo es uno de ellos).
        // Dos vistas (⌥A): los ACTIVOS (sección fija + lista numerada) o el ARCHIVO
        // entero (solo los archivados, con scroll). Nunca las dos: no hay espacio.
        let archivados: Vec<usize> = (0..self.tabs.len())
            .filter(|&i| self.archivados.contains(&self.tabs[i].name) && !fijos.contains(&i))
            .collect();
        let vista_archivo = self.mostrar_archivados;
        if vista_archivo {
            fijos.clear();
        }
        let mut resto: Vec<usize> = if vista_archivo {
            archivados.clone()
        } else {
            (0..self.tabs.len()).filter(|i| !fijos.contains(i) && !archivados.contains(i)).collect()
        };
        // Orden de dibujo (⌥O): los números siguen siendo los reales.
        match self.orden.as_str() {
            "alfa" => resto.sort_by_key(|&i| self.tabs[i].name.to_lowercase()),
            "reciente" => resto.sort_by(|&a, &b| {
                let fa = self.ultimo_foco.get(&self.tabs[a].name).cloned().unwrap_or_default();
                let fb = self.ultimo_foco.get(&self.tabs[b].name).cloned().unwrap_or_default();
                fb.cmp(&fa).then_with(|| a.cmp(&b))
            }),
            "prioridad" => resto.sort_by(|&a, &b| {
                let pa = *self.prioridades.get(&self.tabs[a].name).unwrap_or(&9);
                let pb = *self.prioridades.get(&self.tabs[b].name).unwrap_or(&9);
                pa.cmp(&pb).then_with(|| self.tabs[a].name.to_lowercase().cmp(&self.tabs[b].name.to_lowercase()))
            }),
            _ => {}
        }
        let filas_fijas = if fijos.is_empty() { 0 } else { fijos.len() + 1 };
        let filas_cabecera = if vista_archivo { 2 } else { 0 };
        let available_rows = rows.saturating_sub(top_padding + filas_fijas + filas_cabecera + 1);

        let tab_count = resto.len();
        let active_real = self.active_tab_idx.saturating_sub(1);
        let active_index = resto.iter().position(|&i| i == active_real).unwrap_or(0);

        let (start_index, end_index, tabs_above, tabs_below) =
            calculate_visible_range(tab_count, available_rows, active_index);

        let mut lines: Vec<String> = Vec::with_capacity(rows);
        let mut row_map: Vec<Option<usize>> = Vec::with_capacity(rows);

        // Add top padding lines
        for _ in 0..top_padding {
            lines.push(self.build_empty_line(cols));
            row_map.push(None);
        }

        for &i in &fijos {
            if let Some(tab) = self.tabs.get(i).cloned() {
                let reunion_activo = "#[bg=237,fill]     #[fg=13,bg=237]{name}{indicators}{atencion}".to_string();
                let reunion = "     #[fg=13]{name}{atencion}".to_string();
                let format: &str = if es_reunion(&tab.name) {
                    if tab.active { &reunion_activo } else { &reunion }
                } else if tab.active {
                    &self.style.format_arriba_active
                } else {
                    &self.style.format_arriba
                };
                let styled = self.expand_tmux_format(format, &tab, i + self.style.start_index);
                lines.push(self.build_line(&styled, cols, tab.active));
                row_map.push(Some(i));
            }
        }
        if !fijos.is_empty() {
            lines.push(self.build_line(&parse_styled_string(SEPARADOR), cols, false));
            row_map.push(None);
        }

        // Cabecera de la vista archivo
        if vista_archivo {
            let cab = format!("#[fg=3,bold]ARCHIVO #[fg=dim]{} tab{}", archivados.len(), if archivados.len() == 1 { "" } else { "s" });
            lines.push(self.build_line(&parse_styled_string(&cab), cols, false));
            row_map.push(None);
            lines.push(self.build_line(&parse_styled_string(SEPARADOR), cols, false));
            row_map.push(None);
        }

        // Render "above" overflow indicator
        if tabs_above > 0 {
            let indicator_text =
                self.expand_overflow_format(&self.style.overflow_above, tabs_above);
            let styled = parse_styled_string(&indicator_text);
            lines.push(self.build_line(&styled, cols, false));
            row_map.push(None);
        }

        // Render visible tabs (del resto; `i` es el índice real del tab)
        for &i in resto.iter().take(end_index).skip(start_index) {
            if lines.len() >= rows {
                break;
            }
            if let Some(tab) = self.tabs.get(i).cloned() {
                let is_active = tab.active;
                let format = if is_active {
                    &self.style.format_active
                } else {
                    &self.style.format
                };

                let fmt_arch_activo = "#[bg=237,fill]#[fg=dim,bg=237]{num} {prio} {name}{atencion}".to_string();
                let fmt_arch = "#[fg=dim]{num} {prio} {name}{atencion}".to_string();
                let format = if vista_archivo { if is_active { &fmt_arch_activo } else { &fmt_arch } } else { format };
                let styled = self.expand_tmux_format(format, &tab, i + self.style.start_index);
                lines.push(self.build_line(&styled, cols, is_active));
                row_map.push(Some(i));

                for fila in self.filas_extra(&tab, cols) {
                    if lines.len() >= rows {
                        break;
                    }
                    let styled = parse_styled_string(&fila);
                    lines.push(self.build_line(&styled, cols, false));
                    row_map.push(Some(i));
                }
            }
        }

        // Render "below" overflow indicator
        if tabs_below > 0 {
            let indicator_text =
                self.expand_overflow_format(&self.style.overflow_below, tabs_below);
            let styled = parse_styled_string(&indicator_text);
            lines.push(self.build_line(&styled, cols, false));
            row_map.push(None);
        }

        // Fill remaining rows with empty lines (just border)
        while lines.len() < rows {
            lines.push(self.build_empty_line(cols));
            row_map.push(None);
        }
        // Última fila: el modo de Zellij (sustituye a la status-bar de abajo).
        // Nota: NO cerrar panes desde aquí. close_plugin_pane en 26 instancias a la vez
        // reventó 15 con 'cannot recursively acquire mutex' (8-sep): el host reentra
        // la instancia mientras procesa el pipe. La status-bar se quita por plantilla.
        if rows >= 2 {
            let modo = format!("{:?}", self.mode_info.mode).to_lowercase();
            let fila = if modo == "normal" {
                // mínima: solo los atajos. ⌥A muestra/oculta los archivados (con cuántos hay)
                // siempre visibles: ⌥? ayuda, ⌥A archivados (con cuántos hay, ▾ si desplegados)
                let arch = if self.archivados.is_empty() { "⌥A".to_string() } else { format!("⌥A {}{}", self.archivados.len(), if self.mostrar_archivados { " ▾" } else { "" }) };
                let orden = match self.orden.as_str() { "alfa" => " a-z", "reciente" => " ◷", "prioridad" => " !", _ => "" };
                // columnas visibles: "⌥?" + 2 espacios = 4; luego arch, 2 espacios, ⌥O
                self.col_arch = 4;
                self.col_orden = 4 + arch.chars().count() + 2;
                format!("#[fg=dim]⌥?  {}  ⌥O{}", arch, orden)
            } else {
                format!("#[fg=3,bold]{} #[fg=dim]· ⎋ vuelve", modo.to_uppercase())
            };
            let i = rows - 1;
            lines[i] = self.build_line(&parse_styled_string(&fila), cols, false);
            row_map[i] = None;
        }
        self.row_map = row_map;
        self.ultimas_filas = rows;

        // Print all lines with ANSI styling
        for (i, line) in lines.iter().enumerate() {
            if i < lines.len() - 1 {
                println!("{}\x1b[m", line);
            } else {
                print!("{}\x1b[m", line);
            }
        }
    }

    fn get_tab_at_row(&self, row: usize) -> Option<usize> {
        if self.tabs.is_empty() {
            return None;
        }

        // Con filas de estado bajo los tabs, la fila ya no es el índice: usar el
        // mapa que dejó el último render. Las filas sin tab (indicadores de
        // overflow, relleno) caen al cálculo de abajo.
        if let Some(Some(i)) = self.row_map.get(row) {
            return Some(i + 1);
        }

        let tab_count = self.tabs.len();
        let active_index = self.active_tab_idx.saturating_sub(1);

        let (start_index, end_index, tabs_above, _tabs_below) =
            calculate_visible_range(tab_count, self.last_rows, active_index);

        let content_start_row = if tabs_above > 0 { 1 } else { 0 };

        if tabs_above > 0 && row == 0 {
            let target = start_index.saturating_sub(1);
            return Some(target + 1);
        }

        let row_in_content = row.saturating_sub(content_start_row);
        let clicked_tab_index = start_index + row_in_content;

        if clicked_tab_index < end_index && clicked_tab_index < tab_count {
            return Some(clicked_tab_index + 1);
        }

        if row_in_content >= end_index - start_index {
            let target = end_index.min(tab_count.saturating_sub(1));
            return Some(target + 1);
        }

        None
    }
}

fn calculate_visible_range(
    tab_count: usize,
    available_rows: usize,
    active_index: usize,
) -> (usize, usize, usize, usize) {
    if tab_count == 0 {
        return (0, 0, 0, 0);
    }

    if tab_count <= available_rows {
        return (0, tab_count, 0, 0);
    }

    let max_visible = available_rows.saturating_sub(2);
    if max_visible == 0 {
        return (0, 0, tab_count, 0);
    }

    let mut start_index = active_index;
    let mut end_index = active_index + 1;
    let mut room_left = max_visible.saturating_sub(1);
    let mut alternate = false;

    while room_left > 0 {
        if !alternate && start_index > 0 {
            start_index -= 1;
            room_left -= 1;
        } else if alternate && end_index < tab_count {
            end_index += 1;
            room_left -= 1;
        } else if start_index > 0 {
            start_index -= 1;
            room_left -= 1;
        } else if end_index < tab_count {
            end_index += 1;
            room_left -= 1;
        } else {
            break;
        }
        alternate = !alternate;
    }

    (
        start_index,
        end_index,
        start_index,
        tab_count.saturating_sub(end_index),
    )
}

/// Tabs de reunión: cuelgan de la sección fija de arriba (config `arriba`).
const SEPARADOR: &str = "#[fg=dim]──────────────────────────";
fn es_reunion(nombre: &str) -> bool {
    // tabs de acción que cuelgan de hoy: reuniones (◷), triage/minutas (⚑), capacity (⏱)
    nombre.starts_with('◷') || nombre.starts_with('⚑') || nombre.starts_with('⏱')
}

fn norm_session_name(s: &str) -> String {
    let t = s.trim_start();
    let mut chars = t.chars();
    if let Some(first) = chars.clone().next()
        && !first.is_alphanumeric()
    {
        return chars
            .by_ref()
            .skip(1)
            .collect::<String>()
            .trim_start()
            .to_string();
    }
    t.to_string()
}

fn truncate_string(s: &str, max_width: usize) -> String {
    if s.width() <= max_width {
        return s.to_string();
    }

    if max_width <= 3 {
        return ".".repeat(max_width);
    }

    let mut truncated = String::new();
    let mut width = 0;
    for ch in s.chars() {
        let ch_width = ch.to_string().width();
        if width + ch_width + 3 > max_width {
            truncated.push_str("...");
            break;
        }
        truncated.push(ch);
        width += ch_width;
    }
    truncated
}
