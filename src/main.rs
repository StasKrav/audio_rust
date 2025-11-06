use clap::Parser;
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Paragraph, Gauge, List, ListItem, ListState,
    },
    Terminal,
};
use rodio::{Decoder, OutputStream, Sink, Source};
use std::fs;
use std::fs::File;
use std::io::{self, BufReader};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use std::thread;

#[derive(Parser)]
#[command(name = "Hi-Res Player")]
#[command(about = "Минималистичный аудио-плеер для hi-res форматов")]
struct Cli {
    #[arg(help = "Аудио файл для воспроизведения (опционально)")]
    file: Option<String>,
}

struct App {
    files: Vec<PathBuf>,
    current_file: Option<PathBuf>,
    file_name: String,
    file_format: String,
    sample_rate: u32,
    channels: u16,
    duration: Duration,
    elapsed: Duration,
    playing: bool,
    paused: bool,
    volume: f32,
    start_time: Option<Instant>,
    list_state: ListState,
}

impl App {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        // Получаем список аудио файлов в текущей папке
        let audio_extensions = ["wav", "flac", "mp3", "ogg", "m4a", "aac"];
        let mut files: Vec<PathBuf> = fs::read_dir(".")?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| audio_extensions.contains(&ext.to_lowercase().as_str()))
                    .unwrap_or(false)
            })
            .collect();
        
        files.sort();

        let mut list_state = ListState::default();
        if !files.is_empty() {
            list_state.select(Some(0));
        }

        Ok(App {
            files,
            current_file: None,
            file_name: "Выберите файл".to_string(),
            file_format: "".to_string(),
            sample_rate: 0,
            channels: 0,
            duration: Duration::from_secs(0),
            elapsed: Duration::from_secs(0),
            playing: false,
            paused: false,
            volume: 1.0,
            start_time: None,
            list_state,
        })
    }

    fn load_file(&mut self, file_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let file = File::open(file_path)?;
        let source = Decoder::new(BufReader::new(file))?;
        
        self.file_name = file_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Unknown")
            .to_string();

        self.file_format = file_path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_uppercase();

        self.sample_rate = source.sample_rate();
        self.channels = source.channels();
        self.duration = source.total_duration().unwrap_or(Duration::from_secs(0));
        self.current_file = Some(file_path.to_path_buf());
        
        Ok(())
    }

    fn update_time(&mut self) {
        if let Some(start_time) = self.start_time {
            if self.playing && !self.paused {
                self.elapsed = start_time.elapsed();
                // Не даем времени уйти дальше длительности
                if self.elapsed > self.duration {
                    self.elapsed = self.duration;
                    self.playing = false;
                }
            }
        }
    }

    fn start_playback(&mut self) {
        if !self.playing {
            self.start_time = Some(Instant::now());
            self.playing = true;
            self.paused = false;
        } else if self.paused {
            // Корректируем время при возобновлении
            if let Some(start_time) = self.start_time {
                self.start_time = Some(start_time + self.elapsed);
            }
            self.paused = false;
        }
    }

    fn pause_playback(&mut self) {
        if self.playing && !self.paused {
            self.paused = true;
        }
    }

    fn stop_playback(&mut self) {
        self.playing = false;
        self.paused = false;
        self.elapsed = Duration::from_secs(0);
        self.start_time = None;
    }

    fn next_file(&mut self) {
        if let Some(selected) = self.list_state.selected() {
            if selected < self.files.len() - 1 {
                self.list_state.select(Some(selected + 1));
            }
        }
    }

    fn previous_file(&mut self) {
        if let Some(selected) = self.list_state.selected() {
            if selected > 0 {
                self.list_state.select(Some(selected - 1));
            }
        }
    }

    // fn get_selected_file(&self) -> Option<&PathBuf> {
    //     self.list_state.selected().and_then(|i| self.files.get(i))
    // }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    
    // Создаем приложение
    let mut app = App::new()?;

    // Если файл указан в аргументах, загружаем его
    if let Some(file_path) = cli.file {
        let path = Path::new(&file_path);
        if path.exists() {
            app.load_file(path)?;
            // Находим индекс файла в списке
            if let Some(pos) = app.files.iter().position(|p| p == path) {
                app.list_state.select(Some(pos));
            }
        }
    }

    // Инициализируем аудио систему
    let (_stream, stream_handle) = OutputStream::try_default()?;
    let sink = Sink::try_new(&stream_handle)?;
    sink.pause(); // Начинаем с паузы

    // Настраиваем терминал
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Главный цикл
    'main: loop {
        // Обновляем время воспроизведения
        app.update_time();
        
        // Отрисовываем интерфейс
        terminal.draw(|f| ui(f, &app))?;

        // Обрабатываем ввод
        // Обрабатываем ввод
        // Обрабатываем ввод
        // Обрабатываем ввод
        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break 'main,
                    
                    // Навигация по списку (только выбор)
                    KeyCode::Down => {
                        app.next_file();
                    }
                    KeyCode::Up => {
                        app.previous_file();
                    }
                    
                    // Загрузка выбранного файла
                    KeyCode::Enter => {
                        if let Some(selected_idx) = app.list_state.selected() {
                            if let Some(selected_file) = app.files.get(selected_idx) {
                                let file_path = selected_file.clone();
                                if app.load_file(&file_path).is_ok() {
                                    app.stop_playback();
                                    let file = File::open(&file_path)?;
                                    let source = Decoder::new(BufReader::new(file))?;
                                    sink.append(source);
                                    sink.pause();
                                }
                            }
                        }
                    }
                    
                    // Управление воспроизведением
                    KeyCode::Char(' ') => {
                        if app.current_file.is_some() {
                            if app.paused || !app.playing {
                                app.start_playback();
                                sink.play();
                            } else {
                                app.pause_playback();
                                sink.pause();
                            }
                        }
                    }
                    KeyCode::Char('s') => {
                        app.stop_playback();
                        sink.stop();
                    }
                    KeyCode::Char('r') => {
                        app.stop_playback();
                        if let Some(current_file) = &app.current_file {
                            let file = File::open(current_file)?;
                            let source = Decoder::new(BufReader::new(file))?;
                            sink.append(source);
                            sink.pause();
                        }
                    }
                    
                    // Навигация по трекам с автозагрузкой
                    KeyCode::Right => {
                        app.next_file();
                        if let Some(selected_idx) = app.list_state.selected() {
                            if let Some(selected_file) = app.files.get(selected_idx) {
                                let file_path = selected_file.clone();
                                if app.load_file(&file_path).is_ok() {
                                    app.stop_playback();
                                    let file = File::open(&file_path)?;
                                    let source = Decoder::new(BufReader::new(file))?;
                                    sink.append(source);
                                    sink.pause();
                                    app.start_playback();
                                    sink.play();
                                }
                            }
                        }
                    }
                    KeyCode::Left => {
                        app.previous_file();
                        if let Some(selected_idx) = app.list_state.selected() {
                            if let Some(selected_file) = app.files.get(selected_idx) {
                                let file_path = selected_file.clone();
                                if app.load_file(&file_path).is_ok() {
                                    app.stop_playback();
                                    let file = File::open(&file_path)?;
                                    let source = Decoder::new(BufReader::new(file))?;
                                    sink.append(source);
                                    sink.pause();
                                    app.start_playback();
                                    sink.play();
                                }
                            }
                        }
                    }
                    
                    // Громкость
                    KeyCode::Char('+') => {
                        app.volume = (app.volume + 0.1).min(1.0);
                        sink.set_volume(app.volume);
                    }
                    KeyCode::Char('-') => {
                        app.volume = (app.volume - 0.1).max(0.0);
                        sink.set_volume(app.volume);
                    }
                    _ => {}
                }
            }
        }

        // Проверяем окончание воспроизведения
        if sink.empty() && app.playing {
            app.stop_playback();
        }

        thread::sleep(Duration::from_millis(50));
    }

    // Восстанавливаем терминал
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
    )?;
    terminal.show_cursor()?;

    sink.stop();
    println!("🎵 До свидания!");

    Ok(())
}

fn ui(frame: &mut ratatui::Frame<CrosstermBackend<io::Stdout>>, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(3),  // Заголовок
            Constraint::Percentage(40), // Список файлов
            Constraint::Length(8),  // Информация о треке
            Constraint::Length(3),  // Прогресс-бар
            Constraint::Length(5),  // Управление
        ])
        .split(frame.size());

    // Заголовок
    let title = Paragraph::new("🎵 Hi-Res Audio Player")
        .style(Style::default().fg(Color::LightCyan).add_modifier(Modifier::BOLD))
        .alignment(ratatui::layout::Alignment::Center);
    frame.render_widget(title, chunks[0]);

    // Список файлов
    let files: Vec<ListItem> = app.files
        .iter()
        .enumerate()
        .map(|(i, path)| {
            let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("Unknown");
            let content = Line::from(if Some(i) == app.list_state.selected() {
                Span::styled(
                    format!("▶ {} ", filename),
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                )
            } else {
                Span::styled(
                    format!("  {} ", filename),
                    Style::default().fg(Color::Gray)
                )
            });
            ListItem::new(content)
        })
        .collect();

    let files_list = List::new(files)
        .block(Block::default().borders(Borders::ALL).title(" ФАЙЛЫ "))
        .highlight_style(Style::default().fg(Color::Yellow).bg(Color::DarkGray))
        .highlight_symbol("▶ ");
    frame.render_stateful_widget(files_list, chunks[1], &mut app.list_state.clone());

    // Информация о треке - создаем строки заранее
    let elapsed_str = format_time(app.elapsed);
    let duration_str = format_time(app.duration);
    let format_info = format!(" • {}Hz • {} ch", app.sample_rate, app.channels);
    let status_text = format!("Статус: {}", if app.paused { "⏸️ Пауза" } else if app.playing { "▶️ Воспроизведение" } else { "⏹️ Остановлено" });
    
    let track_info = vec![
        Line::from(vec![
            Span::styled("🎼 ", Style::default().fg(Color::Yellow)),
            Span::styled(&app.file_name, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("🎛  ", Style::default().fg(Color::Yellow)),
            Span::styled(&app.file_format, Style::default().fg(Color::Green)),
            Span::styled(&format_info, Style::default().fg(Color::Gray)),
        ]),
        Line::from(vec![
            Span::styled("⏱  ", Style::default().fg(Color::Yellow)),
            Span::styled(&elapsed_str, Style::default().fg(Color::White)),
            Span::styled(" / ", Style::default().fg(Color::Gray)),
            Span::styled(&duration_str, Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("🎚  ", Style::default().fg(Color::Yellow)),
            Span::styled(&status_text, Style::default().fg(if app.playing { Color::Green } else { Color::Yellow })),
        ]),
    ];

    let info_block = Block::default()
        .borders(Borders::ALL)
        .title(" TRACK INFO ")
        .border_style(Style::default().fg(Color::Blue));
    let info_paragraph = Paragraph::new(track_info).block(info_block);
    frame.render_widget(info_paragraph, chunks[2]);

    // Прогресс-бар
    let progress = if app.duration.as_secs() > 0 {
        app.elapsed.as_secs_f64() / app.duration.as_secs_f64()
    } else {
        0.0
    };

    let gauge = Gauge::default()
        .block(Block::default().borders(Borders::ALL).title(" PROGRESS "))
        .gauge_style(
            Style::default()
                .fg(if app.playing { Color::LightBlue } else { Color::Gray })
                .add_modifier(Modifier::BOLD)
        )
        .percent((progress * 100.0) as u16);
    frame.render_widget(gauge, chunks[3]);

    // Управление - создаем строки заранее
    let volume_str = format!("{:.0}%", app.volume * 100.0);
    let play_pause_text = if app.paused || !app.playing { "▶️ Play" } else { "⏸️ Pause" };
    
    let controls_text = vec![
        Line::from(vec![
            Span::styled("[↑↓] ", Style::default().fg(Color::Gray)),
            Span::styled("Выбор", Style::default().fg(Color::White)),
            Span::styled(" [Enter] ", Style::default().fg(Color::Gray)),
            Span::styled("Загрузить", Style::default().fg(Color::Green)),
            Span::styled(" [←→] ", Style::default().fg(Color::Gray)),
            Span::styled("След/Пред", Style::default().fg(Color::Cyan)),
        ]),
        Line::from(vec![
            Span::styled("[Space] ", Style::default().fg(Color::Gray)),
            Span::styled(play_pause_text, Style::default().fg(Color::Green)),
            Span::styled(" [S] ", Style::default().fg(Color::Gray)),
            Span::styled("⏹️ Stop", Style::default().fg(Color::LightRed)),
            Span::styled(" [R] ", Style::default().fg(Color::Gray)),
            Span::styled("🔄 Restart", Style::default().fg(Color::Yellow)),
        ]),
        Line::from(vec![
            Span::styled("[+]/[-] ", Style::default().fg(Color::Gray)),
            Span::styled("Vol: ", Style::default().fg(Color::Gray)),
            Span::styled(&volume_str, Style::default().fg(Color::White)),
            Span::styled(" [Q] ", Style::default().fg(Color::Gray)),
            Span::styled("🚪 Quit", Style::default().fg(Color::LightRed)),
        ]),
    ];

    let controls_paragraph = Paragraph::new(controls_text)
        .block(Block::default().borders(Borders::ALL).title(" CONTROLS "));
    frame.render_widget(controls_paragraph, chunks[4]);
}

fn format_time(duration: Duration) -> String {
    let total_seconds = duration.as_secs();
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    format!("{:02}:{:02}", minutes, seconds)
}

