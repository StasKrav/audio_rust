mod audio_engine_cpal;
use audio_engine_cpal::{HighQualityPlayer, CpalSymphoniaSource};
use clap::Parser;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    // event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, ListState, Paragraph},
    Terminal,
};

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

// -------- цвета -------
// Цветовая палитра приложения
mod theme {
    use ratatui::style::Color;

    // Основные цвета
    pub const BACKGROUND: Color = Color::Rgb(53, 52, 54); // #0A0C0F - глубокий темный
    pub const SURFACE: Color = Color::Rgb(53, 52, 54); // #14161C - поверхность

    // Акцентные цвета
    pub const PRIMARY: Color = Color::Rgb(190, 116, 190); // #00B8D4 - рамки
    pub const SECONDARY: Color = Color::Rgb(142, 89, 178); // #6496FF - папки
    pub const SUCCESS: Color = Color::Rgb(252, 105, 153); // #4CAF50 - маркированные файлы
    pub const WARNING: Color = Color::Rgb(190, 116, 190); // #FFC107 - текст файла под курсором

    // Текст
    pub const TEXT_PRIMARY: Color = Color::Rgb(240, 240, 240); // #F0F0F0 - основной текст
    pub const TEXT_SECONDARY: Color = Color::Rgb(160, 160, 160); // #B4B4BE - второстепенный
    pub const TEXT_DISABLED: Color = Color::Rgb(80, 80, 80); // #64646E - отключенный

    // Состояния
    // pub const HOVER: Color = Color::Rgb(40, 42, 50);             // #282A32 - при наведении
    pub const SELECTED: Color = Color::Rgb(63, 62, 64); // #1E2028 - выделенный
                                                        // pub const ACTIVE: Color = Color::Rgb(0, 150, 200);           // #0096C8 - активный
}

// Стили для конкретных элементов
mod styles {
    use super::theme;
    use ratatui::style::Style;

    // Панели
    pub fn active_panel() -> Style {
        Style::default().fg(theme::PRIMARY)
    }

    pub fn inactive_panel() -> Style {
        Style::default().fg(theme::TEXT_DISABLED)
    }

    // Выделение
    pub fn highlight_active() -> Style {
        Style::default().fg(theme::WARNING).bg(theme::SELECTED)
    }

    pub fn highlight_inactive() -> Style {
        Style::default()
            .fg(theme::TEXT_DISABLED)
            .bg(theme::BACKGROUND)
    }

    // Элементы
    pub fn folder() -> Style {
        Style::default().fg(theme::SECONDARY)
    }

    pub fn selected_file() -> Style {
        Style::default().fg(theme::SUCCESS)
    }

    pub fn playing_track() -> Style {
        Style::default().fg(theme::SUCCESS)
    }

    pub fn normal_file() -> Style {
        Style::default().fg(theme::TEXT_SECONDARY)
    }
    pub fn inactive_text() -> Style {
        Style::default().fg(theme::TEXT_DISABLED) // Более тусклый цвет
    }

    // Фоны
    pub fn background() -> Style {
        Style::default().bg(theme::BACKGROUND)
    }

    pub fn surface() -> Style {
        Style::default().bg(theme::SURFACE)
    }
}
// ------------------------------------

#[derive(Parser)]
#[command(name = "Hi-Res Player")]
#[command(about = "Файловый менеджер и плеер для hi-res аудио")]
struct Cli {
    #[arg(help = "Начальная папка (опционально)")]
    folder: Option<String>,
}

#[derive(Clone)]
struct FileEntry {
    path: PathBuf,
    is_dir: bool,
    name: String,
    selected: bool,
    duration: Option<std::time::Duration>,
}

struct PlaylistEntry {
    path: PathBuf,
    name: String,
    playing: bool,                         // Добавляем флаг воспроизведения
    duration: Option<std::time::Duration>, // Добавляем длительность
}
fn get_audio_duration(path: &Path) -> Option<std::time::Duration> {
    match CpalSymphoniaSource::new(path) {
        Ok(source) => source.duration(),
        Err(_) => None,
    }
}


fn format_duration(duration: Option<std::time::Duration>) -> String {
    match duration {
        Some(d) => {
            let total_seconds = d.as_secs();
            let minutes = total_seconds / 60;
            let seconds = total_seconds % 60;
            format!("[{:02}:{:02}]", minutes, seconds)
        }
        None => "[--:--]".to_string(),
    }
}
fn format_time(duration: std::time::Duration) -> String {
    let total_seconds = duration.as_secs();
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    format!("{:02}:{:02}", minutes, seconds)
}
struct App {
    current_dir: PathBuf,
    files: Vec<FileEntry>,
    playlist: Vec<PlaylistEntry>,
    files_list_state: ListState,
    playlist_list_state: ListState,
    active_panel: usize,

    audio_player: HighQualityPlayer,
   
    current_playlist_index: usize,
    is_playing: bool,
    current_playing_path: Option<PathBuf>,
    current_playback_position: std::time::Duration,
    playback_start_time: Option<std::time::Instant>,
    save_dialog: Option<SaveDialog>,
    show_help: bool,
    help_scroll: u16,
}
#[derive(Default)]
struct SaveDialog {
    visible: bool,
    filename: String,
    cursor_position: usize, // ВОЗВРАЩАЕМ курсор
    current_dir: PathBuf,
    files: Vec<FileEntry>, // Добавляем список файлов для навигации
    list_state: ListState,
}

fn parse_m3u_file(path: &Path) -> Result<Vec<PlaylistEntry>, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(path)?;
    let mut entries = Vec::new();
    let mut current_extinf: Option<String> = None;
    let base_dir = path.parent().unwrap_or(Path::new("."));

    for line in content.lines() {
        let line = line.trim();

        if line.is_empty() {
            continue;
        }

        if line.starts_with("#EXTM3U") {
            continue; // Пропускаем заголовок
        }

        if line.starts_with("#EXTINF:") {
            current_extinf = Some(line.to_string());
            continue;
        }

        // Это строка с путем к файлу
        if !line.starts_with("#") {
            let file_path = if Path::new(line).is_absolute() {
                PathBuf::from(line)
            } else {
                base_dir.join(line)
            };

            if file_path.exists() {
                let name = if let Some(extinf) = &current_extinf {
                    // Парсим название из #EXTINF
                    if let Some(comma_pos) = extinf.rfind(',') {
                        extinf[comma_pos + 1..].to_string()
                    } else {
                        file_path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("Unknown")
                            .to_string()
                    }
                } else {
                    file_path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("Unknown")
                        .to_string()
                };

                let duration = if file_path
                    .extension()
                    .map_or(false, |ext| is_audio_extension(ext))
                {
                    get_audio_duration(&file_path)
                } else {
                    None
                };

                entries.push(PlaylistEntry {
                    path: file_path,
                    name,
                    playing: false,
                    duration,
                });
            }

            current_extinf = None;
        }
    }

    Ok(entries)
}

// Вспомогательная функция для проверки аудио расширений
fn is_audio_extension(ext: &std::ffi::OsStr) -> bool {
    let audio_extensions = ["wav", "flac", "ogg", "m4a", "aac", "dsf", "dff"];
    ext.to_str()
        .map(|ext| audio_extensions.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}

impl App {
    fn new(start_dir: Option<String>) -> Result<Self, Box<dyn std::error::Error>> {
        let (current_dir, initial_file) = if let Some(dir) = start_dir {
            let path = PathBuf::from(&dir);

            // Пробуем найти файл/папку относительно текущей директории
            let absolute_path = if path.is_absolute() {
                path
            } else {
                std::env::current_dir()?.join(path)
            };

            if absolute_path.exists() {
                if absolute_path.is_dir() {
                    (absolute_path, None)
                } else if absolute_path.is_file() {
                    // Если передан файл - берем его директорию и запоминаем файл
                    let parent = absolute_path
                        .parent()
                        .map(|p| p.to_path_buf())
                        .unwrap_or_else(|| PathBuf::from("."));
                    (parent, Some(absolute_path))
                } else {
                    return Err("Указанный путь не является файлом или папкой".into());
                }
            } else {
                return Err(format!("Путь не существует: {}", absolute_path.display()).into());
            }
        } else {
            // По умолчанию - домашняя директория
            let home_dir = std::env::var("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("/"));
            (home_dir, None)
        };

        // Канонизируем путь (убираем ../ и ./)
        let current_dir = current_dir.canonicalize().unwrap_or(current_dir);

        let audio_player = HighQualityPlayer::new()
            .map_err(|e| format!("Failed to initialize audio player: {}", e))?;

        let mut app = App {
            current_dir,
            files: Vec::new(),
            playlist: Vec::new(),
            files_list_state: ListState::default(),
            playlist_list_state: ListState::default(),
            active_panel: 0,
            audio_player,
            current_playlist_index: 0,
            is_playing: false,
            current_playing_path: None,
            current_playback_position: std::time::Duration::ZERO,
            playback_start_time: None,
            save_dialog: None,
            show_help: false,
            help_scroll: 0,
        };
        app.load_directory()?;

        // Если был передан файл - добавляем его в плейлист и начинаем воспроизведение
        // В методе new(), где добавляем начальный файл в плейлист:
        if let Some(file_path) = initial_file {
            if let Some(file_name) = file_path.file_name().and_then(|n| n.to_str()) {
                let duration = get_audio_duration(&file_path);
                app.playlist.push(PlaylistEntry {
                    path: file_path.clone(),
                    name: file_name.to_string(),
                    playing: false,
                    duration, // Добавляем длительность
                });

                // Начинаем воспроизведение
                app.play()?;
            }
        }

        Ok(app)
    }

    // fn rewind_backward(&mut self) {
    //     // ТОЛЬКО ВИЗУАЛЬНАЯ ПЕРЕМОТКА - не влияет на реальное воспроизведение
    //     if self.is_playing {
    //         let new_pos = self.current_playback_position.saturating_sub(Duration::from_secs(10));
    //         self.current_playback_position = new_pos;
    //         // ОБНОВЛЯЕМ ВРЕМЯ СТАРТА ДЛЯ КОРРЕКТНОГО ПРОДОЛЖЕНИЯ
    //         if let Some(start_time) = self.playback_start_time {
    //             self.playback_start_time = Some(start_time - (self.current_playback_position - new_pos));
    //         }
    //     }
    // }
    //
    // fn rewind_forward(&mut self) {
    //     // ТОЛЬКО ВИЗУАЛЬНАЯ ПЕРЕМОТКА
    //     if self.is_playing {
    //         if let Some(total) = self.get_current_track_duration() {
    //             let new_pos = (self.current_playback_position + Duration::from_secs(10)).min(total);
    //             self.current_playback_position = new_pos;
    //             // ОБНОВЛЯЕМ ВРЕМЯ СТАРТА
    //             if let Some(start_time) = self.playback_start_time {
    //                 self.playback_start_time = Some(start_time - (self.current_playback_position - new_pos));
    //             }
    //         }
    //     }
    // }

    // fn restart_playback_from_seek(&mut self, seek_time: Duration, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    //     // Останавливаем текущее воспроизведение
    //     self.stop();
    //
    //     // СОЗДАЕМ ОДИН ИСТОЧНИК И ДЕЛАЕМ SEEK
    //     let mut source = SymphoniaSource::new(path)?;
    //     source.seek(seek_time)?;
    //
    //     // СОЗДАЕМ RODIO SINK
    //     let (stream, stream_handle) = OutputStream::try_default()?;
    //     let sink = Sink::try_new(&stream_handle)?;
    //
    //     // ИСПОЛЬЗУЕМ ТОТ ЖЕ ИСТОЧНИК ДЛЯ ВОСПРОИЗВЕДЕНИЯ
    //     sink.append(source);
    //     sink.play();
    //
    //     // ОБНОВЛЯЕМ СОСТОЯНИЕ - НЕ СОХРАНЯЕМ source отдельно!
    //     self.current_source = None; // Больше не храним отдельно
    //     self.sink = Some(sink);
    //     self._stream = Some(stream);
    //     self.current_playing_path = Some(path.to_path_buf());
    //     self.is_playing = true;
    //     self.current_playback_position = seek_time;
    //     self.playback_start_time = Some(std::time::Instant::now());
    //
    //     self.update_playing_status();
    //     Ok(())
    // }
    //
    // ОБНОВЛЯЕМ update_playback_progress - ВОЗВРАЩАЕМ РАСЧЕТНОЕ ВРЕМЯ
    fn update_playback_progress(&mut self) {
            if self.is_playing {
                if let Some(start_time) = self.playback_start_time {
                    self.current_playback_position = start_time.elapsed();
                }
            }
        }
    //
    // // УПРОЩАЕМ get_current_track_duration
    // fn get_current_track_duration(&self) -> Option<Duration> {
    //     if let Some(current_path) = &self.current_playing_path {
    //         SymphoniaSource::new(current_path).ok()?.duration()
    //     } else {
    //         None
    //     }
    // }

    // F1 - Показать справку (заглушка)
    fn show_help(&mut self) {
        self.show_help = !self.show_help;
        self.help_scroll = 0; // Сбрасываем скролл при открытии
    }
    // F9 - Сохранить плейлист
    fn show_save_dialog(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let current_dir = std::env::current_dir().unwrap_or_default();
        let mut dialog = SaveDialog {
            visible: true,
            filename: "playlist.m3u".to_string(),
            cursor_position: 11,
            current_dir: current_dir.clone(),
            files: Vec::new(),
            list_state: ListState::default(),
        };

        // Загружаем файлы текущей директории
        dialog.load_directory()?;
        self.save_dialog = Some(dialog);
        Ok(())
    }

    fn hide_save_dialog(&mut self) {
        self.save_dialog = None;
    }

    fn save_playlist(&self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(dialog) = &self.save_dialog {
            let path = if dialog.filename.starts_with('/')
                || dialog.filename.starts_with('\\')
                || (dialog.filename.len() > 2 && dialog.filename.chars().nth(1) == Some(':'))
            {
                // Абсолютный путь
                PathBuf::from(&dialog.filename)
            } else {
                // Относительный путь от текущей директории диалога
                dialog.current_dir.join(&dialog.filename)
            };

            // Создаем директорию если нужно
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            let mut content = String::new();
            content.push_str("#EXTM3U\n");

            for entry in &self.playlist {
                if let Some(duration) = entry.duration {
                    let seconds = duration.as_secs();
                    content.push_str(&format!("#EXTINF:{},{}\n", seconds, entry.name));
                } else {
                    content.push_str(&format!("#EXTINF:-1,{}\n", entry.name));
                }
                content.push_str(&format!("{}\n", entry.path.display()));
            }

            std::fs::write(&path, content)?;
            // println!("✅ Плейлист сохранен: {}", path.display());
        }
        Ok(())
    }

    // Обработка ввода в диалоге сохранения
    fn handle_save_dialog_input(
        &mut self,
        key: event::KeyEvent,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(dialog) = &mut self.save_dialog {
            match key.code {
                KeyCode::Enter => {
                    if let Err(e) = self.save_playlist() {
                        eprintln!("Ошибка сохранения: {}", e);
                    }
                    self.hide_save_dialog();
                }
                KeyCode::Esc => {
                    self.hide_save_dialog();
                }
                KeyCode::Char(c) => {
                    // РАЗРЕШАЕМ ВСЕ СИМВОЛЫ - кириллица будет работать
                    dialog.filename.insert(dialog.cursor_position, c);
                    dialog.cursor_position += 1;
                }
                KeyCode::Backspace => {
                    if dialog.cursor_position > 0 {
                        dialog.cursor_position -= 1;
                        dialog.filename.remove(dialog.cursor_position);
                    }
                }
                KeyCode::Left => {
                    if key.modifiers.contains(KeyModifiers::CONTROL) {
                        // Ctrl+Left - переход в родительскую директорию
                        if let Some(parent) = dialog.current_dir.parent() {
                            dialog.current_dir = parent.to_path_buf();
                            dialog.load_directory()?;
                        }
                    } else {
                        // Обычный Left
                        if dialog.cursor_position > 0 {
                            dialog.cursor_position -= 1;
                        }
                    }
                }
                KeyCode::Right => {
                    if key.modifiers.contains(KeyModifiers::CONTROL) {
                        // Ctrl+Right - вход в директорию
                        dialog.enter_directory()?;
                    } else {
                        // Обычный Right
                        if dialog.cursor_position < dialog.filename.len() {
                            dialog.cursor_position += 1;
                        }
                    }
                }

                KeyCode::Home => {
                    dialog.cursor_position = 0;
                }
                KeyCode::End => {
                    dialog.cursor_position = dialog.filename.len();
                }
                KeyCode::Down => {
                    // Навигация по файлам
                    if let Some(selected) = dialog.list_state.selected() {
                        if selected < dialog.files.len() - 1 {
                            dialog.list_state.select(Some(selected + 1));
                        }
                    } else if !dialog.files.is_empty() {
                        dialog.list_state.select(Some(0));
                    }
                }
                KeyCode::Up => {
                    // Навигация по файлам
                    if let Some(selected) = dialog.list_state.selected() {
                        if selected > 0 {
                            dialog.list_state.select(Some(selected - 1));
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    // F2 - Play
    fn play(&mut self) -> Result<(), Box<dyn std::error::Error>> {
            // Если на паузе - продолжаем
            if self.audio_player.is_playing() && !self.is_playing {
                self.audio_player.resume()?;
                self.is_playing = true;
                self.playback_start_time = Some(std::time::Instant::now() - self.current_playback_position);
                return Ok(());
            }
    
            // Иначе начинаем новое воспроизведение
            self.start_playback()?;
            Ok(())
        }
    
        fn pause(&mut self) {
            if self.audio_player.is_playing() {
                self.audio_player.pause();
                self.is_playing = false;
                // Сохраняем позицию при паузе
                if let Some(start_time) = self.playback_start_time {
                    self.current_playback_position = start_time.elapsed();
                    self.playback_start_time = None;
                }
            }
        }
    
        fn stop(&mut self) {
            self.audio_player.stop();
            self.is_playing = false;
            self.current_playing_path = None;
            self.current_playback_position = std::time::Duration::ZERO;
            self.playback_start_time = None;
            self.update_playing_status();
        }

    fn load_directory(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.files.clear();

        let entries = fs::read_dir(&self.current_dir)?;
        let mut dirs = Vec::new();
        let mut audio_files = Vec::new();

        for entry in entries {
            if let Ok(entry) = entry {
                let path = entry.path();

                // Пропускаем скрытые файлы/папки
                if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                    if file_name.starts_with('.') {
                        continue;
                    }
                }

                let is_dir = path.is_dir();

                if is_dir {
                    dirs.push(FileEntry {
                        path: path.clone(),
                        is_dir: true,
                        name: path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .map(|s| format!("{}/", s))
                            .unwrap_or_else(|| "Unknown/".to_string()),
                        selected: false,
                        duration: None,
                    });
                } else if is_audio_file(&path) || path.extension().map_or(false, |ext| ext == "m3u")
                {
                    let duration = if path.extension().map_or(false, |ext| ext == "m3u") {
                        None // У m3u файлов нет длительности
                    } else {
                        get_audio_duration(&path)
                    };
                    audio_files.push(FileEntry {
                        path: path.clone(),
                        is_dir: false,
                        name: path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("Unknown")
                            .to_string(),
                        selected: false,
                        duration,
                    });
                }
            }
        }

        // Сортируем: сначала папки, потом файлы
        dirs.sort_by(|a, b| a.name.cmp(&b.name));
        audio_files.sort_by(|a, b| a.name.cmp(&b.name));

        self.files.extend(dirs);
        self.files.extend(audio_files);

        // Выбираем первый элемент
        if !self.files.is_empty() {
            self.files_list_state.select(Some(0));
        }

        Ok(())
    }

    fn next_item(&mut self) {
        match self.active_panel {
            0 => {
                if let Some(selected) = self.files_list_state.selected() {
                    if selected < self.files.len() - 1 {
                        self.files_list_state.select(Some(selected + 1));
                    }
                } else if !self.files.is_empty() {
                    self.files_list_state.select(Some(0));
                }
            }
            1 => {
                if let Some(selected) = self.playlist_list_state.selected() {
                    if selected < self.playlist.len() - 1 {
                        self.playlist_list_state.select(Some(selected + 1));
                    }
                } else if !self.playlist.is_empty() {
                    self.playlist_list_state.select(Some(0));
                }
            }
            _ => {}
        }
    }

    fn previous_item(&mut self) {
        match self.active_panel {
            0 => {
                if let Some(selected) = self.files_list_state.selected() {
                    if selected > 0 {
                        self.files_list_state.select(Some(selected - 1));
                    }
                }
            }
            1 => {
                if let Some(selected) = self.playlist_list_state.selected() {
                    if selected > 0 {
                        self.playlist_list_state.select(Some(selected - 1));
                    }
                }
            }
            _ => {}
        }
    }

    fn leave_directory(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.active_panel == 0 {
            if let Some(parent) = self.current_dir.parent() {
                self.current_dir = parent.to_path_buf();
                self.load_directory()?;
            }
        }
        Ok(())
    }

    fn toggle_current_selection(&mut self) {
        if self.active_panel == 0 {
            if let Some(selected) = self.files_list_state.selected() {
                if let Some(entry) = self.files.get_mut(selected) {
                    if !entry.is_dir {
                        entry.selected = !entry.selected;
                    }
                }
            }
        }
    }

    fn move_selected_to_playlist(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.active_panel == 0 {
            let selected_files: Vec<FileEntry> = self
                .files
                .iter()
                .filter(|entry| entry.selected && !entry.is_dir)
                .cloned()
                .collect();

            for file in selected_files {
                if file.path.extension().map_or(false, |ext| ext == "m3u") {
                    // Если это M3U файл - парсим его
                    let m3u_entries = parse_m3u_file(&file.path)?;
                    for m3u_entry in m3u_entries {
                        self.playlist.push(m3u_entry);
                    }
                } else {
                    // Обычный аудиофайл
                    self.playlist.push(PlaylistEntry {
                        path: file.path.clone(),
                        name: file.name.clone(),
                        playing: false,
                        duration: file.duration,
                    });
                }
            }

            // Снимаем выделение после перемещения
            for entry in &mut self.files {
                entry.selected = false;
            }
        }
        Ok(())
    }

    fn handle_right_key(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.active_panel == 0 {
            if let Some(selected) = self.files_list_state.selected() {
                if let Some(entry) = self.files.get(selected) {
                    if entry.is_dir {
                        // Вход в папку
                        self.current_dir = entry.path.clone();
                        self.load_directory()?;
                    } else if entry.path.extension().map_or(false, |ext| ext == "m3u") {
                        // Если это M3U файл - добавляем все треки из плейлиста
                        let m3u_entries = parse_m3u_file(&entry.path)?;
                        for m3u_entry in m3u_entries {
                            self.playlist.push(m3u_entry);
                        }
                    } else {
                        // Перемещение выделенных файлов в плейлист
                        if let Err(e) = self.move_selected_to_playlist() {
                            eprintln!("Ошибка при добавлении в плейлист: {}", e);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn add_to_playlist(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.active_panel == 0 {
            if let Some(selected) = self.files_list_state.selected() {
                if let Some(entry) = self.files.get(selected) {
                    if !entry.is_dir {
                        if entry.path.extension().map_or(false, |ext| ext == "m3u") {
                            // Если это M3U файл - парсим его
                            let m3u_entries = parse_m3u_file(&entry.path)?;
                            for m3u_entry in m3u_entries {
                                self.playlist.push(m3u_entry);
                            }
                        } else {
                            // Обычный аудиофайл
                            self.playlist.push(PlaylistEntry {
                                path: entry.path.clone(),
                                name: entry.name.clone(),
                                playing: false,
                                duration: entry.duration,
                            });
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn remove_from_playlist(&mut self) {
        if self.active_panel == 1 {
            if let Some(selected) = self.playlist_list_state.selected() {
                if selected < self.playlist.len() {
                    let _removed = self.playlist.remove(selected);

                    // Обновляем выделение
                    if self.playlist.is_empty() {
                        self.playlist_list_state.select(None);
                    } else if selected >= self.playlist.len() {
                        self.playlist_list_state
                            .select(Some(self.playlist.len() - 1));
                    }
                }
            }
        }
    }

    // Увеличение громкости
    fn volume_up(&mut self) {
            let new_volume = (self.audio_player.get_volume() + 0.1).min(1.0);
            self.audio_player.set_volume(new_volume);
        }
    
        fn volume_down(&mut self) {
            let new_volume = (self.audio_player.get_volume() - 0.1).max(0.0);
            self.audio_player.set_volume(new_volume);
        }
    fn switch_panel(&mut self) {
        self.active_panel = (self.active_panel + 1) % 2;
    }

    // Переименовываем старый метод play в start_playback
    fn start_playback(&mut self) -> Result<(), Box<dyn std::error::Error>> {
          self.stop();

          let file_to_play = match self.active_panel {
              0 => {
                  if let Some(selected) = self.files_list_state.selected() {
                      self.files
                          .get(selected)
                          .filter(|entry| !entry.is_dir)
                          .map(|entry| entry.path.clone())
                  } else {
                      None
                  }
              }
              1 => {
                  if let Some(selected) = self.playlist_list_state.selected() {
                      self.playlist.get(selected).map(|entry| entry.path.clone())
                  } else {
                      None
                  }
              }
              _ => None,
          };

          if let Some(path) = file_to_play {
              let source = CpalSymphoniaSource::new(&path)?;
              
              self.audio_player.play_source(&source)?;
              
              self.current_playing_path = Some(path);
              self.is_playing = true;
              self.current_playback_position = std::time::Duration::ZERO;
              self.playback_start_time = Some(std::time::Instant::now());

              self.update_playing_status();
          }

          Ok(())
      }

    // fn next_track(&mut self) -> Result<(), Box<dyn std::error::Error>> {
    //     // println!("⏭️ Следующий трек");
    //     self.play_next() // <-- Использовать правильный метод
    // }

//     fn previous_track(&mut self) -> Result<(), Box<dyn std::error::Error>> {
//         if self.current_playlist_index > 0 {
//             self.current_playlist_index -= 1;
// 
//             if let Some(sink) = &self.sink {
//                 sink.stop();
//             }
// 
//             let files_to_play: Vec<PathBuf> = self
//                 .playlist
//                 .iter()
//                 .map(|entry| entry.path.clone())
//                 .collect();
// 
//             if self.current_playlist_index < files_to_play.len() {
//                 if let Some(prev_file) = files_to_play.get(self.current_playlist_index) {
//                     self.current_playing_path = Some(prev_file.clone());
// 
//                     let file = File::open(prev_file)?;
//                     let source = Decoder::new(BufReader::new(file))?;
// 
//                     let (stream, stream_handle) = OutputStream::try_default()?;
//                     let sink = Sink::try_new(&stream_handle)?;
//                     sink.append(source);
//                     sink.play();
// 
//                     self._stream = Some(stream);
//                     self.sink = Some(sink);
//                     self.is_playing = true;
// 
//                     // СБРАСЫВАЕМ И ЗАПУСКАЕМ ПРОГРЕСС ДЛЯ ПРЕДЫДУЩЕГО ТРЕКА
//                     self.current_playback_position = std::time::Duration::ZERO;
//                     self.playback_start_time = Some(std::time::Instant::now());
// 
//                     self.update_playing_status();
//                 }
//             }
//         }
// 
//         Ok(())
//     }

    fn update_playing_status(&mut self) {
        // Сбрасываем статус playing у всех треков
        for entry in &mut self.playlist {
            entry.playing = false;
        }

        // Помечаем текущий играющий трек
        if let Some(current_path) = &self.current_playing_path {
            for entry in &mut self.playlist {
                if &entry.path == current_path {
                    entry.playing = true;
                    break;
                }
            }
        }
    }

//     fn play_next(&mut self) -> Result<(), Box<dyn std::error::Error>> {
//         if let Some(sink) = &self.sink {
//             sink.stop();
//         }
// 
//         self.current_playlist_index += 1;
// 
//         // Определяем следующий файл для воспроизведения
//         let files_to_play: Vec<PathBuf> = self
//             .playlist
//             .iter()
//             .map(|entry| entry.path.clone())
//             .collect();
// 
//         // Проверяем есть ли еще треки
//         if self.current_playlist_index >= files_to_play.len() {
//             self.is_playing = false;
//             self.current_playlist_index = 0;
//             self.current_playing_path = None;
//             // СБРАСЫВАЕМ ПРОГРЕСС
//             self.current_playback_position = std::time::Duration::ZERO;
//             self.playback_start_time = None;
//             self.update_playing_status();
//             return Ok(());
//         }
// 
//         // Воспроизводим следующий трек
//         if let Some(next_file) = files_to_play.get(self.current_playlist_index) {
//             self.current_playing_path = Some(next_file.clone());
// 
//             let file = File::open(next_file)?;
//             let source = Decoder::new(BufReader::new(file))?;
// 
//             let (stream, stream_handle) = OutputStream::try_default()?;
//             let sink = Sink::try_new(&stream_handle)?;
//             sink.append(source);
//             sink.play();
// 
//             self._stream = Some(stream);
//             self.sink = Some(sink);
//             self.is_playing = true;
// 
//             // СБРАСЫВАЕМ И ЗАПУСКАЕМ ПРОГРЕСС ДЛЯ НОВОГО ТРЕКА
//             self.current_playback_position = std::time::Duration::ZERO;
//             self.playback_start_time = Some(std::time::Instant::now());
// 
//             self.update_playing_status();
//         }
// 
//         Ok(())
//     }

    fn check_playback_finished(&mut self) {
            if self.is_playing && self.audio_player.samples_remaining() == 0 {
                // Просто останавливаем воспроизведение
                self.stop();
            }
        }
    
}
impl SaveDialog {
    fn load_directory(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.files.clear();

        let entries = match fs::read_dir(&self.current_dir) {
            Ok(entries) => entries,
            Err(e) => {
                eprintln!(
                    "Ошибка чтения директории {}: {}",
                    self.current_dir.display(),
                    e
                );
                return Ok(()); // Возвращаемся без ошибки, чтобы не падать
            }
        };

        let mut dirs = Vec::new();
        let mut files = Vec::new();

        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => continue, // Пропускаем проблемные файлы
            };

            let path = entry.path();

            // Безопасное получение имени файла
            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(file_name) => {
                    // Пропускаем скрытые файлы/папки
                    if file_name.starts_with('.') {
                        continue;
                    }
                    if path.is_dir() {
                        format!("{}/", file_name)
                    } else {
                        file_name.to_string()
                    }
                }
                None => continue, // Пропускаем файлы с некорректными именами
            };

            let is_dir = path.is_dir();

            if is_dir {
                dirs.push(FileEntry {
                    path: path.clone(),
                    is_dir: true,
                    name,
                    selected: false,
                    duration: None,
                });
            } else {
                files.push(FileEntry {
                    path: path.clone(),
                    is_dir: false,
                    name,
                    selected: false,
                    duration: None,
                });
            }
        }

        // Сортируем: сначала папки, потом файлы
        dirs.sort_by(|a, b| a.name.cmp(&b.name));
        files.sort_by(|a, b| a.name.cmp(&b.name));

        self.files.extend(dirs);
        self.files.extend(files);

        // Выбираем первый элемент
        if !self.files.is_empty() {
            self.list_state.select(Some(0));
        }

        Ok(())
    }

    fn enter_directory(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(selected) = self.list_state.selected() {
            if let Some(entry) = self.files.get(selected) {
                if entry.is_dir {
                    self.current_dir = entry.path.clone();
                    if let Err(e) = self.load_directory() {
                        eprintln!("Ошибка загрузки директории: {}", e);
                        // Возвращаемся обратно при ошибке
                        if let Some(parent) = self.current_dir.parent() {
                            self.current_dir = parent.to_path_buf();
                        }
                    }
                }
            }
        }
        Ok(())
    }
} // <-- Закрывающая фигурная скобка для impl SaveDialog
fn is_audio_file(path: &Path) -> bool {
    let audio_extensions = [
        "wav", "flac", "ogg", "m4a", "aac", "dsf", "dff", "m3u",
    ];
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| audio_extensions.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}
// Добавляем функцию центрирования ПОСЛЕ функции ui
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Увеличиваем размер аудиобуфера для предотвращения underrun
    env::set_var("RUST_AUDIO_BACKEND_BUFFER_SIZE", "8192");
    env::set_var("RUST_AUDIO_LATENCY", "1");
    let cli = Cli::parse();

    // Создаем приложение
    let mut app = App::new(cli.folder)?;

    // Настраиваем терминал
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Главный цикл
    'main: loop {
        // ОБНОВЛЯЕМ ПРОГРЕСС ВОСПРОИЗВЕДЕНИЯ
        app.update_playback_progress();
        // Проверяем окончание воспроизведения
        app.check_playback_finished();

        // Отрисовываем интерфейс
        terminal.draw(|f| ui(f, &app))?;

        // ★★★ ОБРАБОТКА ДИАЛОГА ★★★
        // В главном цикле, где обрабатывается диалог:
        // В главном цикле, где обрабатывается диалог:
        if let Some(dialog) = &app.save_dialog {
            if dialog.visible {
                match event::read()? {
                    Event::Key(key) => {
                        if let Err(e) = app.handle_save_dialog_input(key) {
                            eprintln!("Ошибка в диалоге сохранения: {}", e);
                            // Не закрываем приложение при ошибке, просто логируем
                        }
                    }
                    _ => {}
                }
                continue;
            }
        }

        // Обрабатываем ввод
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    _ if app.show_help => {
                        match key.code {
                            KeyCode::Esc => app.show_help = false,
                            KeyCode::Down | KeyCode::Char('j') => {
                                app.help_scroll = app.help_scroll.saturating_add(1);
                            }
                            KeyCode::Up | KeyCode::Char('k') => {
                                app.help_scroll = app.help_scroll.saturating_sub(1);
                            }
                            KeyCode::PageDown => {
                                app.help_scroll = app.help_scroll.saturating_add(10);
                            }
                            KeyCode::PageUp => {
                                app.help_scroll = app.help_scroll.saturating_sub(10);
                            }
                            KeyCode::Home => {
                                app.help_scroll = 0;
                            }
                            KeyCode::End => {
                                app.help_scroll = u16::MAX;
                            }
                            _ => {} // Игнорируем другие клавиши в справке
                        }
                    }

                    // Группа 1: Основное управление (F1-F4)
                    KeyCode::F(1) => app.show_help(),
                    KeyCode::F(2) => {
                        if let Err(e) = app.play() {
                            eprintln!("Ошибка воспроизведения: {}", e);
                        }
                    }
                    KeyCode::F(3) => app.pause(),
                    KeyCode::F(4) => app.stop(),

                    // Группа 2: Навигация по трекам (F5-F8)
                    // KeyCode::F(5) => {
                    //     if let Err(e) = app.previous_track() {
                    //         eprintln!("Ошибка переключения трека: {}", e);
                    //     }
                    // }
                    // KeyCode::F(6) => {
                    //     if let Err(e) = app.next_track() {
                    //         eprintln!("Ошибка переключения трека: {}", e);
                    //     }
                    // }
                    // KeyCode::F(7) => {
                    //     if let Err(e) = app.rewind_backward() {
                    //         eprintln!("Ошибка перемотки назад: {}", e);
                    //     }
                    // }
                    // KeyCode::F(8) => {
                    //     if let Err(e) = app.rewind_forward() {
                    //         eprintln!("Ошибка перемотки вперед: {}", e);
                    //     }
                    // }
                    KeyCode::Char('q') | KeyCode::Esc => break 'main,
                    KeyCode::Tab => app.switch_panel(),
                    KeyCode::F(9) => {
                        if app.save_dialog.is_none() {
                            if let Err(e) = app.show_save_dialog() {
                                eprintln!("Ошибка открытия диалога сохранения: {}", e);
                            }
                        } else {
                            app.hide_save_dialog();
                        }
                    }

                    // Громкость
                    KeyCode::Char('+') => {
                        app.volume_up();
                    }
                    KeyCode::Char('-') => {
                        app.volume_down();
                    }

                    // Навигация и выделение
                    KeyCode::Down => {
                        if key.modifiers.contains(KeyModifiers::CONTROL) {
                            app.toggle_current_selection();
                            app.next_item();
                        } else {
                            app.next_item();
                        }
                    }
                    KeyCode::Up => {
                        if key.modifiers.contains(KeyModifiers::CONTROL) {
                            app.toggle_current_selection();
                            app.previous_item();
                        } else {
                            app.previous_item();
                        }
                    }
                    KeyCode::Right => {
                        if let Err(e) = app.handle_right_key() {
                            eprintln!("Ошибка: {}", e);
                        }
                    }
                    KeyCode::Left => {
                        if let Err(e) = app.leave_directory() {
                            eprintln!("Ошибка: {}", e);
                        }
                    }

                    // В match key.code { ... } добавь:
                    // KeyCode::Esc => {
                    //     if app.show_help {
                    //         app.show_help = false; // Закрыть справку по Esc
                    //     } else {
                    //         break 'main; // Выйти из приложения
                    //     }
                    // }

                    // Действия
                    KeyCode::Enter => {
                        if let Err(e) = app.add_to_playlist() {
                            eprintln!("Ошибка добавления в плейлист: {}", e);
                        }
                    }
                    KeyCode::Delete => app.remove_from_playlist(),

                    _ => {}
                }
            }
        }
    }

    // Восстанавливаем терминал
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen,)?;
    terminal.show_cursor()?;

    println!("🎵 До свидания!");
    Ok(())
}

fn ui(frame: &mut ratatui::Frame<CrosstermBackend<io::Stdout>>, app: &App) {
    // use theme::*;
    use styles::*;

    // Фон всего приложения
    frame.render_widget(Block::default().style(background()), frame.size());

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),    // Основная область (панели)
            Constraint::Length(2), // Две пустые строки (разделитель)
            Constraint::Length(3), // Статусная строка
        ])
        .split(frame.size());

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[0]);

    // Файловый менеджер - разделяем на заголовок, пустую строку и контент
    let files_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Заголовок
            Constraint::Length(1), // Пустая строка (разделитель)
            Constraint::Min(1),    // Список файлов
        ])
        .split(columns[0]);
    // Рендерим диалог сохранения поверх основного интерфейса
    // Рендерим диалог сохранения поверх основного интерфейса

    // Рендерим заголовок файлового менеджера
    let files_title_style = if app.active_panel == 0 {
        active_panel()
    } else {
        inactive_panel()
    };

    let files_title = Paragraph::new(Line::from(Span::styled(
        " FILE MANAGER ",
        files_title_style,
    )))
    .style(surface());
    frame.render_widget(files_title, files_chunks[0]);

    // Рендерим пустую строку-разделитель
    let empty_line = Paragraph::new("").style(surface());
    frame.render_widget(empty_line, files_chunks[1]);

    // Рендерим список файлов вручную для контроля выравнивания
    let files_area = files_chunks[2];
    let mut y = 0;

    // Вычисляем смещение для скроллинга
    let files_scroll_offset = if let Some(selected) = app.files_list_state.selected() {
        let visible_items = files_area.height as usize;
        if selected >= visible_items {
            selected - visible_items + 1
        } else {
            0
        }
    } else {
        0
    };

    // Рендерим только видимые элементы
    for (i, entry) in app.files.iter().enumerate().skip(files_scroll_offset) {
        if y >= files_area.height as usize {
            break;
        }

        let icon = if entry.is_dir { " " } else { " " };
        let selection_indicator = if entry.selected { " ●" } else { "  " };

        let duration_text = if entry.is_dir {
            "".to_string()
        } else if entry.path.extension().map_or(false, |ext| ext == "m3u") {
            "".to_string() // Для M3U файлов не показываем длительность
        } else {
            format_duration(entry.duration)
        };

        // Вычисляем оригинальный индекс для подсветки
        let original_index = i;

        let style = if app.active_panel == 0 {
            if Some(original_index) == app.files_list_state.selected() {
                Style::default()
                    .fg(theme::TEXT_PRIMARY)
                    .add_modifier(Modifier::BOLD)
            } else if entry.selected {
                selected_file()
            } else if entry.is_dir {
                folder()
            } else {
                normal_file()
            }
        } else {
            styles::inactive_text()
        };

        let duration_style = if app.active_panel == 0 {
            if Some(original_index) == app.files_list_state.selected() {
                Style::default()
                    .fg(theme::TEXT_PRIMARY)
                    .add_modifier(Modifier::BOLD)
            } else if entry.selected {
                selected_file()
            } else if entry.is_dir {
                folder()
            } else {
                normal_file()
            }
        } else {
            styles::inactive_text()
        };
        // Создаем Rect для текущей строки
        let line_rect = Rect::new(files_area.x, files_area.y + y as u16, files_area.width, 1);

        // Разделяем строку на левую и правую части
        let line_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Min(0),    // Левая часть - имя файла
                Constraint::Length(7), // Правая часть - длительность
            ])
            .split(line_rect);

        // Левая часть - имя файла
        let name_text = format!("{}{}{}", selection_indicator, icon, entry.name);
        let name_paragraph =
            Paragraph::new(Line::from(Span::styled(name_text, style))).style(surface());
        frame.render_widget(name_paragraph, line_chunks[0]);

        // Правая часть - длительность (выровнена по правому краю)
        if !entry.is_dir {
            let duration_paragraph =
                Paragraph::new(Line::from(Span::styled(duration_text, duration_style)))
                    .style(surface())
                    .alignment(ratatui::layout::Alignment::Right);
            frame.render_widget(duration_paragraph, line_chunks[1]);
        }

        y += 1;
    }

    // Подсветка выбранного элемента (только если он видим)
    if let Some(selected) = app.files_list_state.selected() {
        if selected >= files_scroll_offset
            && (selected - files_scroll_offset) < files_area.height as usize
        {
            let highlight_y = (selected - files_scroll_offset) as u16;
            let highlight_rect = Rect::new(
                files_area.x,
                files_area.y + highlight_y,
                files_area.width,
                1,
            );
            let highlight = Paragraph::new("").style(if app.active_panel == 0 {
                styles::highlight_active()
            } else {
                styles::highlight_inactive()
            });
            frame.render_widget(highlight, highlight_rect);
        }
    }
    // Плейлист - аналогично разделяем на заголовок, пустую строку и контент
    let playlist_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Заголовок
            Constraint::Length(1), // Пустая строка (разделитель)
            Constraint::Min(1),    // Список плейлиста
        ])
        .split(columns[1]);

    // Рендерим заголовок плейлиста
    let playlist_title_style = if app.active_panel == 1 {
        styles::active_panel()
    } else {
        styles::inactive_panel()
    };

    let playlist_title =
        Paragraph::new(Line::from(Span::styled(" PLAYLIST ", playlist_title_style)))
            .style(styles::surface());
    frame.render_widget(playlist_title, playlist_chunks[0]);

    // Рендерим пустую строку-разделитель для плейлиста
    let empty_line_playlist = Paragraph::new("").style(styles::surface());
    frame.render_widget(empty_line_playlist, playlist_chunks[1]);

    // Рендерим список плейлиста
    // Рендерим плейлист вручную для контроля выравнивания
    // Рендерим плейлист вручную для контроля выравнивания
    let playlist_area = playlist_chunks[2];
    let mut y = 0;

    // Вычисляем смещение для скроллинга
    let playlist_scroll_offset = if let Some(selected) = app.playlist_list_state.selected() {
        let visible_items = playlist_area.height as usize;
        if selected >= visible_items {
            selected - visible_items + 1
        } else {
            0
        }
    } else {
        0
    };

    // Рендерим только видимые элементы
    for (i, entry) in app.playlist.iter().enumerate().skip(playlist_scroll_offset) {
        if y >= playlist_area.height as usize {
            break;
        }

        let icon = if entry.playing { "▶ " } else { " " };
        let selection_indicator = "  ";

        let duration_text = if entry.path.extension().map_or(false, |ext| ext == "m3u") {
            "".to_string() // Для M3U файлов в плейлисте не показываем длительность
        } else {
            format_duration(entry.duration)
        };

        // Вычисляем оригинальный индекс для подсветки
        let original_index = i;

        let style = if app.active_panel == 1 {
            if Some(original_index) == app.playlist_list_state.selected() {
                Style::default()
                    .fg(theme::TEXT_PRIMARY)
                    .add_modifier(Modifier::BOLD)
            } else if entry.playing {
                styles::playing_track()
            } else {
                styles::normal_file()
            }
        } else {
            styles::inactive_text()
        };

        // В цикле рендеринга плейлиста замените стиль для длительности:

        let duration_style = if app.active_panel == 1 {
            if Some(original_index) == app.playlist_list_state.selected() {
                Style::default()
                    .fg(theme::TEXT_PRIMARY)
                    .add_modifier(Modifier::BOLD)
            } else if entry.playing {
                styles::playing_track()
            } else {
                styles::normal_file()
            }
        } else {
            styles::inactive_text()
        };

        // Создаем Rect для текущей строки
        let line_rect = Rect::new(
            playlist_area.x,
            playlist_area.y + y as u16,
            playlist_area.width,
            1,
        );

        // Разделяем строку на левую и правую части
        let line_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Min(0),    // Левая часть - имя трека
                Constraint::Length(7), // Правая часть - длительность
            ])
            .split(line_rect);

        // Левая часть - имя трека
        let name_text = format!("{}{}{}", selection_indicator, icon, entry.name);
        let name_paragraph =
            Paragraph::new(Line::from(Span::styled(name_text, style))).style(styles::surface());
        frame.render_widget(name_paragraph, line_chunks[0]);

        // Правая часть - длительность (выровнена по правому краю)
        let duration_paragraph =
            Paragraph::new(Line::from(Span::styled(duration_text, duration_style)))
                .style(styles::surface())
                .alignment(ratatui::layout::Alignment::Right);
        frame.render_widget(duration_paragraph, line_chunks[1]);

        y += 1;
    }

    // Подсветка выбранного элемента в плейлисте (только если он видим)
    if let Some(selected) = app.playlist_list_state.selected() {
        if selected >= playlist_scroll_offset
            && (selected - playlist_scroll_offset) < playlist_area.height as usize
        {
            let highlight_y = (selected - playlist_scroll_offset) as u16;
            let highlight_rect = Rect::new(
                playlist_area.x,
                playlist_area.y + highlight_y,
                playlist_area.width,
                1,
            );
            let highlight = Paragraph::new("").style(if app.active_panel == 1 {
                styles::highlight_active()
            } else {
                styles::highlight_inactive()
            });
            frame.render_widget(highlight, highlight_rect);
        }
    }

    // Подсветка выбранного элемента в плейлисте
    if let Some(selected) = app.playlist_list_state.selected() {
        if selected < app.playlist.len() && (selected as u16) < playlist_area.height {
            let highlight_rect = Rect::new(
                playlist_area.x,
                playlist_area.y + selected as u16,
                playlist_area.width,
                1,
            );
            let highlight = Paragraph::new("").style(if app.active_panel == 1 {
                styles::highlight_active()
            } else {
                styles::highlight_inactive()
            });
            frame.render_widget(highlight, highlight_rect);
        }
    }

    // Подсветка выбранного элемента
    if let Some(selected) = app.files_list_state.selected() {
        if selected < app.files.len() && (selected as u16) < files_area.height {
            let highlight_rect = Rect::new(
                files_area.x,
                files_area.y + selected as u16,
                files_area.width,
                1,
            );
            let highlight = Paragraph::new("").style(if app.active_panel == 0 {
                styles::highlight_active()
            } else {
                styles::highlight_inactive()
            });
            frame.render_widget(highlight, highlight_rect);
        }
    }

    // Рендерим разделитель (две пустые строки) между панелями и статусной строкой
    let separator = Paragraph::new("").style(background());
    frame.render_widget(separator, chunks[1]);

    // Статусная строка внизу
    let status_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50), // Левая часть - текущий трек + управление плейлистом
            Constraint::Percentage(50), // Правая часть - состояние воспроизведения
        ])
        .split(chunks[2]);

    // Левая часть - текущий трек и управление плейлистом
    let left_status_text = if let Some(current_path) = &app.current_playing_path {
        if let Some(file_name) = current_path.file_name().and_then(|n| n.to_str()) {
            // Обрезаем длинные названия
            let display_name = if file_name.len() > 30 {
                format!("{}...", &file_name[..27])
            } else {
                file_name.to_string()
            };
            format!(" Now: {} ", display_name)
        } else {
            " No track ".to_string()
        }
    } else {
        " No track ".to_string()
    };

    let left_status_paragraph = Paragraph::new(Line::from(vec![
        Span::styled(
            &left_status_text,
            Style::default()
                .fg(theme::TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " [F9]Save ",
            Style::default()
                .fg(theme::TEXT_SECONDARY)
                .add_modifier(Modifier::DIM),
        ),
    ]))
    .style(styles::surface());

    frame.render_widget(left_status_paragraph, status_chunks[0]);

    // Правая часть - состояние воспроизведения
    // В функции ui(), заменяем текущую status_text на:

    // Получаем общую длительность текущего трека
    // В функции ui(), заменяем весь блок прогресс-бара на:

    // Получаем общую длительность текущего трека
    let total_duration = if let Some(current_path) = &app.current_playing_path {
        get_audio_duration(current_path)
    } else {
        None
    };

    // Создаем прогресс-бар
    let (filled, empty, current_time, total_time) = if let (Some(total), Some(current)) =
        (total_duration, Some(app.current_playback_position))
    {
        let progress_ratio = if total.as_secs() > 0 {
            current.as_secs_f64() / total.as_secs_f64()
        } else {
            0.0
        };

        let progress_ratio = progress_ratio.min(1.0);
        let bar_width = 20;
        let filled = (progress_ratio * bar_width as f64).round() as usize;
        let empty = bar_width - filled;

        (filled, empty, format_time(current), format_time(total))
    } else {
        (0, 20, "--:--".to_string(), "--:--".to_string())
    };

    // Объединяем с информацией о состоянии
// В функции ui() замените:
let volume_text = format!("{:.0}%", app.audio_player.get_volume() * 100.0);

let status_icon = if app.audio_player.is_playing() {
    "▶ "
} else {
    "⏹ "
};
    // Создаем цветной прогресс-бар с Spans
    let status_line = Line::from(vec![
        Span::raw(status_icon),
        Span::styled("●".repeat(filled), Style::default().fg(theme::PRIMARY)), // ЗАПОЛНЕННЫЕ - цветные
        Span::styled("◦".repeat(empty), Style::default().fg(theme::TEXT_DISABLED)), // ПУСТЫЕ - серые
        Span::raw(format!(
            " {}/{} | 🔊 {}",
            current_time, total_time, volume_text
        )),
    ]);

    let status_paragraph = Paragraph::new(status_line)
        .style(styles::surface())
        .alignment(ratatui::layout::Alignment::Right);

    frame.render_widget(status_paragraph, status_chunks[1]);

    // ------------ диалоговое окно ---------------------------
    if let Some(dialog) = &app.save_dialog {
        if dialog.visible {
            let overlay = Rect::new(0, 0, frame.size().width, frame.size().height);

            // 1. Очищаем область
            frame.render_widget(Clear, overlay);

            // 2. Рисуем непрозрачный фон
            let background = Block::default().style(Style::default().bg(theme::BACKGROUND));
            frame.render_widget(background, overlay);

            // 3. Рисуем диалог
            let dialog_area = centered_rect(70, 60, frame.size());
            let dialog_block = Block::default()
                .style(styles::active_panel())
                .borders(ratatui::widgets::Borders::NONE)
                .border_style(styles::active_panel())
                .title(" Save Playlist ");
            frame.render_widget(dialog_block, dialog_area);

            let inner_chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints([
                    Constraint::Length(1), // [0] Текущий путь
                    Constraint::Length(3), // [1] Поле ввода
                    Constraint::Min(10),   // [2] Список файлов
                    Constraint::Length(1), // [3] ПУСТАЯ СТРОКА-РАЗДЕЛИТЕЛЬ
                    Constraint::Length(1), // [4] Подсказки
                ])
                .split(dialog_area);

            // Текущий путь
            let path_text = Paragraph::new(format!("Path: {}", dialog.current_dir.display()))
                .style(Style::default().fg(theme::TEXT_DISABLED));
            frame.render_widget(path_text, inner_chunks[0]);

            // Поле ввода с курсором
            let input_area = inner_chunks[1];

            // Создаем текст с видимым курсором
            let input_text = {
                let mut spans = Vec::new();
                let chars: Vec<char> = dialog.filename.chars().collect();

                // Добавляем текст до курсора
                if dialog.cursor_position > 0 {
                    spans.push(Span::styled(
                        chars[..dialog.cursor_position].iter().collect::<String>(),
                        Style::default().fg(theme::SUCCESS),
                    ));
                }

                // Добавляем курсор (инвертированный символ)
                if dialog.cursor_position < chars.len() {
                    spans.push(Span::styled(
                        chars[dialog.cursor_position].to_string(),
                        Style::default().fg(theme::BACKGROUND).bg(theme::SUCCESS),
                    ));

                    // Добавляем оставшийся текст
                    if dialog.cursor_position < chars.len() - 1 {
                        spans.push(Span::styled(
                            chars[dialog.cursor_position + 1..]
                                .iter()
                                .collect::<String>(),
                            Style::default().fg(theme::TEXT_PRIMARY),
                        ));
                    }
                } else {
                    // Курсор в конце - добавляем пробел с инвертированным стилем
                    spans.push(Span::styled(
                        " ",
                        Style::default()
                            .fg(theme::BACKGROUND)
                            .bg(theme::TEXT_PRIMARY),
                    ));
                }

                Line::from(spans)
            };

            let input = Paragraph::new(input_text).style(styles::surface()).block(
                Block::default()
                    .borders(ratatui::widgets::Borders::NONE)
                    .title("File name "),
            );
            frame.render_widget(input, input_area);

            // СПИСОК ФАЙЛОВ С ПРАВИЛЬНЫМ СКРОЛЛИНГОМ
            let files_area = inner_chunks[2];
            let mut y = 0;

            // ВЫЧИСЛЯЕМ СКРОЛЛ ДИНАМИЧЕСКИ (как в основном интерфейсе)
            let scroll_offset = if let Some(selected) = dialog.list_state.selected() {
                let visible_items = files_area.height as usize;
                if selected >= visible_items {
                    selected - visible_items + 1
                } else {
                    0
                }
            } else {
                0
            };

            // Рендерим только видимые элементы
            for (i, entry) in dialog.files.iter().enumerate().skip(scroll_offset) {
                if y >= files_area.height as usize {
                    break;
                }

                // Вычисляем оригинальный индекс для подсветки
                let original_index = i;

                let style = if Some(original_index) == dialog.list_state.selected() {
                    Style::default()
                        .fg(theme::TEXT_PRIMARY)
                        .add_modifier(Modifier::BOLD)
                } else if entry.is_dir {
                    styles::folder()
                } else {
                    styles::normal_file()
                };

                let line_rect =
                    Rect::new(files_area.x, files_area.y + y as u16, files_area.width, 1);
                let name_paragraph = Paragraph::new(Line::from(Span::styled(&entry.name, style)))
                    .style(styles::surface());
                frame.render_widget(name_paragraph, line_rect);

                y += 1;
            }

            // ПОДСВЕТКА ВЫБРАННОГО ЭЛЕМЕНТА (только если он видим)
            if let Some(selected) = dialog.list_state.selected() {
                if selected >= scroll_offset
                    && (selected - scroll_offset) < files_area.height as usize
                {
                    let highlight_y = (selected - scroll_offset) as u16;
                    let highlight_rect = Rect::new(
                        files_area.x,
                        files_area.y + highlight_y,
                        files_area.width,
                        1,
                    );
                    let highlight = Paragraph::new("").style(Style::default().bg(theme::SELECTED));
                    frame.render_widget(highlight, highlight_rect);
                }
            }
            // ПУСТАЯ СТРОКА-РАЗДЕЛИТЕЛЬ
            let separator_area = inner_chunks[3];
            let separator = Paragraph::new("").style(styles::surface());
            frame.render_widget(separator, separator_area);

            // Подсказки
            let hints = Paragraph::new(Line::from(vec![
                Span::styled("Enter: Save  ", Style::default().fg(theme::TEXT_SECONDARY)),
                Span::styled(" Esc: Cancel  ", Style::default().fg(theme::TEXT_SECONDARY)),
                Span::styled(
                    " Ctrl+←/→: Navigate ",
                    Style::default().fg(theme::TEXT_SECONDARY),
                ),
            ]));
            frame.render_widget(hints, inner_chunks[4]);
        }
    }
    // РЕНДЕРИМ СПРАВКУ ЕСЛИ ОНА ВКЛЮЧЕНА
    if app.show_help {
        let help_area = centered_rect(60, 70, frame.size());

        frame.render_widget(Clear, help_area);

        let help_block = Block::default()
            .style(styles::surface())
            .borders(ratatui::widgets::Borders::ALL)
            .border_style(styles::active_panel())
            .title(" Справка Hi-Res Player ");
        frame.render_widget(help_block, help_area);

        let inner_chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Min(1),    // Текст справки со скроллом
                Constraint::Length(1), // Подсказка
            ])
            .split(help_area);

        // УВЕЛИЧИВАЕМ ТЕКСТ СПРАВКИ
        // Текст справки
        let help_text = vec![
            Line::from(""),
            Line::from(vec![Span::styled(
                "Основное управление:",
                Style::default()
                    .fg(theme::PRIMARY)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
            Line::from(vec![
                Span::raw("  "),
                Span::styled("F2", Style::default().fg(theme::SUCCESS)),
                Span::raw(" - Воспроизведение"),
            ]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled("F3", Style::default().fg(theme::SUCCESS)),
                Span::raw(" - Пауза"),
            ]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled("F4", Style::default().fg(theme::SUCCESS)),
                Span::raw(" - Стоп"),
            ]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled("F5", Style::default().fg(theme::SUCCESS)),
                Span::raw(" - Предыдущий трек"),
            ]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled("F6", Style::default().fg(theme::SUCCESS)),
                Span::raw(" - Следующий трек"),
            ]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled("F9", Style::default().fg(theme::SUCCESS)),
                Span::raw(" - Сохранить плейлист"),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Навигация:",
                Style::default()
                    .fg(theme::PRIMARY)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
            Line::from(vec![
                Span::raw("  "),
                Span::styled("Tab", Style::default().fg(theme::WARNING)),
                Span::raw(" - Переключение между панелями"),
            ]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled("↑/↓", Style::default().fg(theme::WARNING)),
                Span::raw(" - Навигация по спискам"),
            ]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled("→", Style::default().fg(theme::WARNING)),
                Span::raw(" - Войти в папку / Добавить в плейлист"),
            ]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled("←", Style::default().fg(theme::WARNING)),
                Span::raw(" - Выйти из папки"),
            ]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled("Enter", Style::default().fg(theme::WARNING)),
                Span::raw(" - Добавить в плейлист"),
            ]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled("Delete", Style::default().fg(theme::WARNING)),
                Span::raw(" - Удалить из плейлиста"),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Громкость:",
                Style::default()
                    .fg(theme::PRIMARY)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
            Line::from(vec![
                Span::raw("  "),
                Span::styled("+", Style::default().fg(theme::SECONDARY)),
                Span::raw(" - Увеличить громкость"),
            ]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled("-", Style::default().fg(theme::SECONDARY)),
                Span::raw(" - Уменьшить громкость"),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Форматы:",
                Style::default()
                    .fg(theme::PRIMARY)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
            Line::from("  FLAC, WAV, OGG, M4A, AAC, DSD (DSF, DFF)"),
            Line::from(""),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Управление в справке:",
                Style::default()
                    .fg(theme::PRIMARY)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
            Line::from(vec![
                Span::raw("  "),
                Span::styled("↑/↓", Style::default().fg(theme::WARNING)),
                Span::raw(" - Прокрутка"),
            ]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled("PageUp/PageDown", Style::default().fg(theme::WARNING)),
                Span::raw(" - Быстрая прокрутка"),
            ]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled("Home/End", Style::default().fg(theme::WARNING)),
                Span::raw(" - В начало/конец"),
            ]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled("Esc", Style::default().fg(theme::WARNING)),
                Span::raw(" - Закрыть справку"),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Поддерживаемые форматы:",
                Style::default()
                    .fg(theme::PRIMARY)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
            Line::from("  • FLAC - формат без потерь"),
            Line::from("  • WAV - несжатый аудио"),
            Line::from("  • OGG/Vorbis - открытый формат"),
            Line::from("  • M4A/AAC - современный формат"),
            Line::from("  • DSD (DSF, DFF) - Super Audio CD"),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Особенности:",
                Style::default()
                    .fg(theme::PRIMARY)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
            Line::from("  • Hi-Res аудио поддержка"),
            Line::from("  • Плейлисты M3U"),
            Line::from("  • Текстовый интерфейс (TUI)"),
            Line::from("  • Написан на Rust"),
            Line::from(""),
            Line::from("Для качественного звука рекомендуется:"),
            Line::from("  • Использовать FLAC или WAV"),
            Line::from("  • Отключить системные эффекты"),
            Line::from("  • Использовать качественные наушники/колонки"),
            Line::from(""),
            Line::from("Приятного прослушивания! 🎵"),
            Line::from(""),
        ];

        let help_paragraph = Paragraph::new(help_text)
            .style(styles::surface())
            .alignment(ratatui::layout::Alignment::Left)
            .scroll((app.help_scroll, 0)); // ← ДОБАВЛЯЕМ СКРОЛЛ

        frame.render_widget(help_paragraph, inner_chunks[0]);

        // Обновляем подсказку
        let hint_text = if app.help_scroll > 0 {
            format!(
                " ↑/↓ - прокрутка | Esc - закрыть (позиция: {}) ",
                app.help_scroll
            )
        } else {
            " ↑/↓ - прокрутка | Esc - закрыть ".to_string()
        };

        let hint = Paragraph::new(Line::from(Span::styled(
            hint_text,
            Style::default().fg(theme::TEXT_SECONDARY),
        )));
        frame.render_widget(hint, inner_chunks[1]);
    }
}
