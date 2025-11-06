use clap::Parser;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, List, ListItem, ListState, Paragraph, Gauge,
    },
    Terminal,
};
use rodio::{Decoder, OutputStream, Sink};
use std::fs;
use std::fs::File;
use std::io::{self, BufReader};
use std::path::{Path, PathBuf};
use std::time::Duration;

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
}

struct PlaylistEntry {
    path: PathBuf,
    name: String,
    playing: bool,
}

struct App {
    current_dir: PathBuf,
    files: Vec<FileEntry>,
    playlist: Vec<PlaylistEntry>,
    files_list_state: ListState,
    playlist_list_state: ListState,
    active_panel: usize,
    _stream: Option<OutputStream>,
    sink: Option<Sink>,
    current_playlist_index: usize,
    is_playing: bool,
}

impl App {
    fn new(start_dir: Option<String>) -> Result<Self, Box<dyn std::error::Error>> {
        let (current_dir, initial_file) = if let Some(dir) = start_dir {
            let path = PathBuf::from(&dir);
            let absolute_path = if path.is_absolute() {
                path
            } else {
                std::env::current_dir()?.join(path)
            };

            if absolute_path.exists() {
                if absolute_path.is_dir() {
                    (absolute_path, None)
                } else if absolute_path.is_file() {
                    let parent = absolute_path.parent()
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
            let home_dir = std::env::var("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("/"));
            (home_dir, None)
        };

        let current_dir = current_dir.canonicalize().unwrap_or(current_dir);
        
        let mut app = App {
            current_dir,
            files: Vec::new(),
            playlist: Vec::new(),
            files_list_state: ListState::default(),
            playlist_list_state: ListState::default(),
            active_panel: 0,
            _stream: None,
            sink: None,
            current_playlist_index: 0,
            is_playing: false,
        };
        
        app.load_directory()?;
        
        if let Some(file_path) = initial_file {
            if let Some(file_name) = file_path.file_name().and_then(|n| n.to_str()) {
                app.playlist.push(PlaylistEntry {
                    path: file_path.clone(),
                    name: file_name.to_string(),
                    playing: false,
                });
                app.play()?;
            }
        }
        
        Ok(app)
    }

    fn load_directory(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.files.clear();

        let entries = fs::read_dir(&self.current_dir)?;
        let mut dirs = Vec::new();
        let mut audio_files = Vec::new();

        for entry in entries {
            if let Ok(entry) = entry {
                let path = entry.path();
                
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
                        name: path.file_name()
                            .and_then(|n| n.to_str())
                            .map(|s| format!("{}/", s))
                            .unwrap_or_else(|| "Unknown/".to_string()),
                        selected: false,
                    });
                } else if is_audio_file(&path) {
                    audio_files.push(FileEntry {
                        path: path.clone(),
                        is_dir: false,
                        name: path.file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("Unknown")
                            .to_string(),
                        selected: false,
                    });
                }
            }
        }

        dirs.sort_by(|a, b| a.name.cmp(&b.name));
        audio_files.sort_by(|a, b| a.name.cmp(&b.name));
        
        self.files.extend(dirs);
        self.files.extend(audio_files);

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

    fn enter_directory(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.active_panel == 0 {
            if let Some(selected) = self.files_list_state.selected() {
                if let Some(entry) = self.files.get(selected) {
                    if entry.is_dir {
                        self.current_dir = entry.path.clone();
                        self.load_directory()?;
                    }
                }
            }
        }
        Ok(())
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

    fn move_selected_to_playlist(&mut self) {
        if self.active_panel == 0 {
            let selected_files: Vec<FileEntry> = self.files
                .iter()
                .filter(|entry| entry.selected && !entry.is_dir)
                .cloned()
                .collect();
            
            for file in selected_files {
                self.playlist.push(PlaylistEntry {
                    path: file.path.clone(),
                    name: file.name.clone(),
                    playing: false,
                });
            }
            
            for entry in &mut self.files {
                entry.selected = false;
            }
        }
    }

    fn handle_right_key(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.active_panel == 0 {
            if let Some(selected) = self.files_list_state.selected() {
                if let Some(entry) = self.files.get(selected) {
                    if entry.is_dir {
                        self.current_dir = entry.path.clone();
                        self.load_directory()?;
                    } else {
                        self.move_selected_to_playlist();
                    }
                }
            }
        }
        Ok(())
    }

    fn add_to_playlist(&mut self) {
        if self.active_panel == 0 {
            if let Some(selected) = self.files_list_state.selected() {
                if let Some(entry) = self.files.get(selected) {
                    if !entry.is_dir {
                        self.playlist.push(PlaylistEntry {
                            path: entry.path.clone(),
                            name: entry.name.clone(),
                            playing: false,
                        });
                    }
                }
            }
        }
    }

    fn remove_from_playlist(&mut self) {
        if self.active_panel == 1 {
            if let Some(selected) = self.playlist_list_state.selected() {
                if selected < self.playlist.len() {
                    let _removed = self.playlist.remove(selected);
                    
                    if self.playlist.is_empty() {
                        self.playlist_list_state.select(None);
                    } else if selected >= self.playlist.len() {
                        self.playlist_list_state.select(Some(self.playlist.len() - 1));
                    }
                }
            }
        }
    }

    fn switch_panel(&mut self) {
        self.active_panel = (self.active_panel + 1) % 2;
    }

    fn play(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(sink) = &self.sink {
            sink.stop();
        }

        self.current_playlist_index = 0;

        let files_to_play = match self.active_panel {
            0 => {
                if self.has_selected_files() {
                    self.get_selected_files()
                } else {
                    if let Some(selected) = self.files_list_state.selected() {
                        if let Some(entry) = self.files.get(selected) {
                            if !entry.is_dir {
                                vec![entry.path.clone()]
                            } else {
                                vec![]
                            }
                        } else {
                            vec![]
                        }
                    } else {
                        vec![]
                    }
                }
            }
            1 => {
                self.playlist.iter().map(|entry| entry.path.clone()).collect()
            }
            _ => vec![],
        };

        if files_to_play.is_empty() {
            return Ok(());
        }

        let (stream, stream_handle) = OutputStream::try_default()?;
        let sink = Sink::try_new(&stream_handle)?;
        
        if let Some(first_file) = files_to_play.get(self.current_playlist_index) {
            let file = File::open(first_file)?;
            let source = Decoder::new(BufReader::new(file))?;
            sink.append(source);
            sink.play();
            
            self._stream = Some(stream);
            self.sink = Some(sink);
            self.is_playing = true;
            self.current_playlist_index = 0;
            self.update_playing_status();
        }

        Ok(())
    }

    fn toggle_playback(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(sink) = &self.sink {
            if sink.is_paused() {
                sink.play();
                self.is_playing = true;
            } else {
                sink.pause();
                self.is_playing = false;
            }
        } else {
            self.play()?;
        }
        
        self.update_playing_status();
        Ok(())
    }

    fn stop_playback(&mut self) {
        if let Some(sink) = &self.sink {
            sink.stop();
        }
        self.is_playing = false;
        self.current_playlist_index = 0;
        self.update_playing_status();
    }

    fn next_track(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.play_next()
    }

    fn previous_track(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.current_playlist_index > 0 {
            self.current_playlist_index -= 1;
            if let Some(sink) = &self.sink {
                sink.stop();
            }
            self.play()?;
        }
        Ok(())
    }

    fn volume_up(&mut self) {
        if let Some(sink) = &self.sink {
            let new_volume = (sink.volume() + 0.1).min(1.0);
            sink.set_volume(new_volume);
        }
    }

    fn volume_down(&mut self) {
        if let Some(sink) = &self.sink {
            let new_volume = (sink.volume() - 0.1).max(0.0);
            sink.set_volume(new_volume);
        }
    }

    fn has_selected_files(&self) -> bool {
        self.files.iter().any(|entry| entry.selected && !entry.is_dir)
    }

    fn get_selected_files(&self) -> Vec<PathBuf> {
        self.files
            .iter()
            .filter(|entry| entry.selected && !entry.is_dir)
            .map(|entry| entry.path.clone())
            .collect()
    }

    fn update_playing_status(&mut self) {
        for entry in &mut self.playlist {
            entry.playing = false;
        }
        
        if self.is_playing && self.current_playlist_index < self.playlist.len() {
            if let Some(entry) = self.playlist.get_mut(self.current_playlist_index) {
                entry.playing = true;
            }
        }
    }

    fn play_next(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(sink) = &self.sink {
            sink.stop();
        }

        self.current_playlist_index += 1;
        
        let files_to_play = match self.active_panel {
            0 => {
                if self.has_selected_files() {
                    self.get_selected_files()
                } else if let Some(selected) = self.files_list_state.selected() {
                    if let Some(entry) = self.files.get(selected) {
                        if !entry.is_dir {
                            vec![entry.path.clone()]
                        } else {
                            vec![]
                        }
                    } else {
                        vec![]
                    }
                } else {
                    vec![]
                }
            }
            1 => {
                self.playlist.iter().map(|entry| entry.path.clone()).collect()
            }
            _ => vec![],
        };

        if self.current_playlist_index >= files_to_play.len() {
            self.is_playing = false;
            self.current_playlist_index = 0;
            self.update_playing_status();
            return Ok(());
        }

        if let Some(next_file) = files_to_play.get(self.current_playlist_index) {
            let file = File::open(next_file)?;
            let source = Decoder::new(BufReader::new(file))?;
            
            let (stream, stream_handle) = OutputStream::try_default()?;
            let sink = Sink::try_new(&stream_handle)?;
            sink.append(source);
            sink.play();
            
            self._stream = Some(stream);
            self.sink = Some(sink);
            self.is_playing = true;
            self.update_playing_status();
        }

        Ok(())
    }

    fn check_playback_finished(&mut self) {
        if let Some(sink) = &self.sink {
            if sink.empty() && self.is_playing {
                if let Err(e) = self.play_next() {
                    eprintln!("Ошибка: {}", e);
                    self.is_playing = false;
                }
            }
        }
    }
} // КОНЕЦ impl App

fn is_audio_file(path: &Path) -> bool {
    let audio_extensions = ["wav", "flac", "mp3", "ogg", "m4a", "aac", "dsf", "dff"];
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| audio_extensions.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    
    let mut app = App::new(cli.folder)?;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    'main: loop {
        app.check_playback_finished();
        
        terminal.draw(|f| ui(f, &app))?;

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break 'main,
                    KeyCode::Tab => app.switch_panel(),
                    KeyCode::Char(' ') => {
                        if let Err(e) = app.toggle_playback() {
                            eprintln!("Ошибка воспроизведения: {}", e);
                        }
                    },
                    KeyCode::Char('s') | KeyCode::Char('S') => {
                        app.stop_playback();
                    },
                    KeyCode::Char('n') | KeyCode::Char('N') => {
                        if let Err(e) = app.next_track() {
                            eprintln!("Ошибка переключения трека: {}", e);
                        }
                    },
                    KeyCode::Char('p') | KeyCode::Char('P') => {
                        if let Err(e) = app.previous_track() {
                            eprintln!("Ошибка переключения трека: {}", e);
                        }
                    },
                    KeyCode::Char('+') => {
                        app.volume_up();
                    },
                    KeyCode::Char('-') => {
                        app.volume_down();
                    },
                    KeyCode::Down => {
                        if key.modifiers.contains(KeyModifiers::CONTROL) {
                            app.toggle_current_selection();
                            app.next_item();
                        } else {
                            app.next_item();
                        }
                    },
                    KeyCode::Up => {
                        if key.modifiers.contains(KeyModifiers::CONTROL) {
                            app.toggle_current_selection();
                            app.previous_item();
                        } else {
                            app.previous_item();
                        }
                    },
                    KeyCode::Right => {
                        if let Err(e) = app.handle_right_key() {
                            eprintln!("Ошибка: {}", e);
                        }
                    },
                    KeyCode::Left => {
                        if let Err(e) = app.leave_directory() {
                            eprintln!("Ошибка: {}", e);
                        }
                    },
                    KeyCode::Enter => app.add_to_playlist(),
                    KeyCode::Delete => app.remove_from_playlist(),
                    _ => {}
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
    )?;
    terminal.show_cursor()?;

    println!("🎵 До свидания!");
    Ok(())
}

fn ui(frame: &mut ratatui::Frame<CrosstermBackend<io::Stdout>>, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(3),
        ])
        .split(frame.size());

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Percentage(50),
        ])
        .split(chunks[0]);

    // Файловый менеджер
    let files: Vec<ListItem> = app.files
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let icon = if entry.is_dir { "📁 " } else { "○ " };
            let selection_indicator = if entry.selected { "█ " } else { "  " };
            
            let style = if app.active_panel == 0 && Some(i) == app.files_list_state.selected() {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else if entry.selected {
                Style::default().fg(Color::Green)
            } else if entry.is_dir {
                Style::default().fg(Color::Blue)
            } else {
                Style::default().fg(Color::Gray)
            };

            let content = Line::from(vec![
                Span::styled(selection_indicator, style),
                Span::styled(icon, style),
                Span::styled(&entry.name, style),
            ]);
            
            ListItem::new(content)
        })
        .collect();

    let files_block_style = if app.active_panel == 0 {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::Gray)
    };

    let files_list = List::new(files)
        .block(Block::default().borders(Borders::ALL).title(" ФАЙЛОВЫЙ МЕНЕДЖЕР ").border_style(files_block_style))
        .highlight_style(Style::default().fg(Color::Yellow).bg(Color::DarkGray));
    
    frame.render_stateful_widget(files_list, columns[0], &mut app.files_list_state.clone());

    // Плейлист
    let playlist: Vec<ListItem> = app.playlist
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let icon = if entry.playing { "▶ " } else { "○ " };
            let style = if app.active_panel == 1 && Some(i) == app.playlist_list_state.selected() {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else if entry.playing {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::Gray)
            };

            let content = Line::from(vec![
                Span::styled(icon, style),
                Span::styled(&entry.name, style),
            ]);
            
            ListItem::new(content)
        })
        .collect();

    let playlist_block_style = if app.active_panel == 1 {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::Gray)
    };

    let playlist_list = List::new(playlist)
        .block(Block::default().borders(Borders::ALL).title(" ПЛЕЙЛИСТ ").border_style(playlist_block_style))
        .highlight_style(Style::default().fg(Color::Yellow).bg(Color::DarkGray));
    
    frame.render_stateful_widget(playlist_list, columns[1], &mut app.playlist_list_state.clone());

    // Статус-бар
    let status_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(chunks[1]);

    // Прогресс-бар (заглушка)
    let progress = 0.5;
    let gauge = Gauge::default()
        .block(Block::default().borders(Borders::NONE))
        .gauge_style(Style::default().fg(Color::LightBlue))
        .percent((progress * 100.0) as u16);
    frame.render_widget(gauge, status_chunks[0]);

    // Информация
    let status_text = if app.is_playing {
        "▶ Воспроизведение | Vol: 100%".to_string()
    } else {
        "⏸ Остановлено | Выберите трек".to_string()
    };

    let status_paragraph = Paragraph::new(status_text)
        .block(Block::default().borders(Borders::ALL).title(" СТАТУС "));
    frame.render_widget(status_paragraph, status_chunks[1]);
}
