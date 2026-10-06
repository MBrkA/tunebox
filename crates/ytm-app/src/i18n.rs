//! UI language. English strings are the keys: `t("Play")` returns the translation for the current
//! language, or the English text when there is none. Only app chrome is translated; titles and
//! names that come from YouTube are shown as served.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Tr,
    De,
    Es,
    Fr,
    /// Mandarin, Simplified Chinese.
    Zh,
}

impl Lang {
    pub const ALL: [Lang; 6] = [Lang::En, Lang::Tr, Lang::De, Lang::Es, Lang::Fr, Lang::Zh];

    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Tr => "tr",
            Lang::De => "de",
            Lang::Es => "es",
            Lang::Fr => "fr",
            Lang::Zh => "zh",
        }
    }

    /// The `hl` value YouTube expects for this language.
    pub fn hl(self) -> &'static str {
        match self {
            Lang::Zh => "zh-CN",
            other => other.code(),
        }
    }

    /// The language's own name, shown in the selector.
    pub fn native_name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Tr => "Türkçe",
            Lang::De => "Deutsch",
            Lang::Es => "Español",
            Lang::Fr => "Français",
            Lang::Zh => "中文（简体）",
        }
    }

    pub fn from_code(code: &str) -> Lang {
        let code = code.trim().to_ascii_lowercase();
        Lang::ALL
            .into_iter()
            .find(|l| code.starts_with(l.code()))
            .unwrap_or(Lang::En)
    }

    fn index(self) -> usize {
        self as usize
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

pub fn set(lang: Lang) {
    CURRENT.store(lang as u8, Ordering::Relaxed);
}

pub fn current() -> Lang {
    Lang::ALL[CURRENT.load(Ordering::Relaxed) as usize % Lang::ALL.len()]
}

/// Translate an English UI string into the current language.
pub fn t(s: &'static str) -> &'static str {
    let lang = current();
    if lang == Lang::En {
        return s;
    }
    static MAP: OnceLock<HashMap<&'static str, [&'static str; 5]>> = OnceLock::new();
    let map = MAP.get_or_init(|| TABLE.iter().map(|(k, v)| (*k, *v)).collect());
    match map.get(s) {
        Some(tr) => tr[lang.index() - 1],
        None => s,
    }
}

/// English → [Türkçe, Deutsch, Español, Français, 简体中文].
#[rustfmt::skip]
const TABLE: &[(&str, [&str; 5])] = &[
    ("Home", ["Ana sayfa", "Start", "Inicio", "Accueil", "首页"]),
    ("Explore", ["Keşfet", "Entdecken", "Explorar", "Explorer", "探索"]),
    ("Library", ["Kitaplık", "Mediathek", "Biblioteca", "Bibliothèque", "媒体库"]),
    ("Playlists", ["Çalma listeleri", "Playlists", "Listas", "Playlists", "播放列表"]),
    ("Settings", ["Ayarlar", "Einstellungen", "Ajustes", "Paramètres", "设置"]),
    ("Language", ["Dil", "Sprache", "Idioma", "Langue", "语言"]),
    ("About", ["Hakkında", "Info", "Acerca de", "À propos", "关于"]),
    ("Version", ["Sürüm", "Version", "Versión", "Version", "版本"]),
    ("Developed by", ["Geliştiren", "Entwickelt von", "Desarrollado por", "Développé par", "开发者"]),
    ("Interface language", ["Arayüz dili", "Oberflächensprache", "Idioma de la interfaz", "Langue de l’interface", "界面语言"]),
    ("Changes apply immediately.", ["Değişiklik hemen uygulanır.", "Änderungen werden sofort übernommen.", "Los cambios se aplican al instante.", "Les changements s’appliquent immédiatement.", "更改立即生效。"]),
    ("Back", ["Geri", "Zurück", "Atrás", "Retour", "返回"]),
    ("Clear", ["Temizle", "Leeren", "Borrar", "Effacer", "清除"]),
    ("Search songs, albums, artists", ["Şarkı, albüm, sanatçı ara", "Songs, Alben, Künstler suchen", "Buscar canciones, álbumes, artistas", "Rechercher titres, albums, artistes", "搜索歌曲、专辑、艺人"]),
    ("Welcome to Tunebox", ["Tunebox'a hoş geldin", "Willkommen bei Tunebox", "Te damos la bienvenida a Tunebox", "Bienvenue sur Tunebox", "欢迎使用 Tunebox"]),
    ("Nothing to show yet.", ["Henüz gösterilecek bir şey yok.", "Noch nichts anzuzeigen.", "Aún no hay nada que mostrar.", "Rien à afficher pour le moment.", "暂无内容。"]),
    ("New releases", ["Yeni çıkanlar", "Neuerscheinungen", "Novedades", "Nouveautés", "新发行"]),
    ("Charts", ["Listeler", "Charts", "Listas de éxitos", "Classements", "排行榜"]),
    ("Moods & genres", ["Ruh halleri ve türler", "Stimmungen & Genres", "Estados de ánimo y géneros", "Ambiances et genres", "心情和流派"]),
    ("See all", ["Tümünü gör", "Alle anzeigen", "Ver todo", "Tout afficher", "查看全部"]),
    ("Could not load this page", ["Sayfa yüklenemedi", "Seite konnte nicht geladen werden", "No se pudo cargar la página", "Impossible de charger la page", "无法加载此页面"]),
    ("Could not load this album", ["Albüm yüklenemedi", "Album konnte nicht geladen werden", "No se pudo cargar el álbum", "Impossible de charger l’album", "无法加载此专辑"]),
    ("Could not load this artist", ["Sanatçı yüklenemedi", "Künstler konnte nicht geladen werden", "No se pudo cargar el artista", "Impossible de charger l’artiste", "无法加载此艺人"]),
    ("Could not load this playlist", ["Çalma listesi yüklenemedi", "Playlist konnte nicht geladen werden", "No se pudo cargar la lista", "Impossible de charger la playlist", "无法加载此播放列表"]),
    ("Could not load moods & genres", ["Ruh halleri ve türler yüklenemedi", "Stimmungen & Genres konnten nicht geladen werden", "No se pudieron cargar los estados de ánimo y géneros", "Impossible de charger les ambiances et genres", "无法加载心情和流派"]),
    ("Could not load this category", ["Kategori yüklenemedi", "Kategorie konnte nicht geladen werden", "No se pudo cargar la categoría", "Impossible de charger la catégorie", "无法加载此分类"]),
    ("Could not load the charts", ["Listeler yüklenemedi", "Charts konnten nicht geladen werden", "No se pudieron cargar las listas", "Impossible de charger les classements", "无法加载排行榜"]),
    ("Try again", ["Tekrar dene", "Erneut versuchen", "Reintentar", "Réessayer", "重试"]),
    ("Play", ["Oynat", "Wiedergabe", "Reproducir", "Lecture", "播放"]),
    ("Pause", ["Duraklat", "Pause", "Pausa", "Pause", "暂停"]),
    ("Shuffle", ["Karıştır", "Zufall", "Aleatorio", "Aléatoire", "随机播放"]),
    ("Radio", ["Radyo", "Radio", "Radio", "Radio", "电台"]),
    ("Add to queue", ["Kuyruğa ekle", "Zur Warteschlange", "Añadir a la cola", "Ajouter à la file", "添加到队列"]),
    ("Top songs", ["En çok dinlenenler", "Top-Songs", "Canciones populares", "Titres populaires", "热门歌曲"]),
    ("All", ["Tümü", "Alle", "Todo", "Tout", "全部"]),
    ("Songs", ["Şarkılar", "Songs", "Canciones", "Titres", "歌曲"]),
    ("Videos", ["Videolar", "Videos", "Vídeos", "Vidéos", "视频"]),
    ("Albums", ["Albümler", "Alben", "Álbumes", "Albums", "专辑"]),
    ("Artists", ["Sanatçılar", "Künstler", "Artistas", "Artistes", "艺人"]),
    ("Artist", ["Sanatçı", "Künstler", "Artista", "Artiste", "艺人"]),
    ("Search failed", ["Arama başarısız", "Suche fehlgeschlagen", "La búsqueda falló", "Échec de la recherche", "搜索失败"]),
    ("No results", ["Sonuç yok", "Keine Ergebnisse", "Sin resultados", "Aucun résultat", "没有结果"]),
    ("Load more", ["Daha fazla yükle", "Mehr laden", "Cargar más", "Charger plus", "加载更多"]),
    ("Now playing", ["Şimdi çalıyor", "Aktuelle Wiedergabe", "Reproduciendo", "En cours de lecture", "正在播放"]),
    ("Nothing playing", ["Çalan bir şey yok", "Keine Wiedergabe", "Nada en reproducción", "Aucune lecture", "未在播放"]),
    ("Close", ["Kapat", "Schließen", "Cerrar", "Fermer", "关闭"]),
    ("Lyrics", ["Şarkı sözleri", "Songtext", "Letra", "Paroles", "歌词"]),
    ("Could not load lyrics.", ["Şarkı sözleri yüklenemedi.", "Songtext konnte nicht geladen werden.", "No se pudo cargar la letra.", "Impossible de charger les paroles.", "无法加载歌词。"]),
    ("No lyrics available for this track.", ["Bu parça için şarkı sözü yok.", "Für diesen Titel ist kein Songtext verfügbar.", "No hay letra para esta canción.", "Pas de paroles pour ce titre.", "此曲目暂无歌词。"]),
    ("Remove from liked songs", ["Beğenilenlerden kaldır", "Aus „Gefällt mir“ entfernen", "Quitar de me gusta", "Retirer des titres aimés", "从喜欢的歌曲中移除"]),
    ("Add to liked songs", ["Beğenilenlere ekle", "Zu „Gefällt mir“ hinzufügen", "Añadir a me gusta", "Ajouter aux titres aimés", "添加到喜欢的歌曲"]),
    ("Queue", ["Kuyruk", "Warteschlange", "Cola", "File d’attente", "队列"]),
    ("Close queue", ["Kuyruğu kapat", "Warteschlange schließen", "Cerrar cola", "Fermer la file", "关闭队列"]),
    ("Save as playlist", ["Çalma listesi olarak kaydet", "Als Playlist speichern", "Guardar como lista", "Enregistrer en playlist", "保存为播放列表"]),
    ("Your queue is empty. Play something to fill it.", ["Kuyruğun boş. Doldurmak için bir şey çal.", "Deine Warteschlange ist leer. Spiele etwas ab.", "Tu cola está vacía. Reproduce algo para llenarla.", "Votre file est vide. Lancez un titre pour la remplir.", "队列为空。播放内容即可填充。"]),
    ("Next up", ["Sıradakiler", "Als Nächstes", "A continuación", "À suivre", "接下来播放"]),
    ("Remove from queue", ["Kuyruktan kaldır", "Aus Warteschlange entfernen", "Quitar de la cola", "Retirer de la file", "从队列中移除"]),
    ("Play now", ["Şimdi oynat", "Jetzt abspielen", "Reproducir ahora", "Lire maintenant", "立即播放"]),
    ("Change cover", ["Kapağı değiştir", "Cover ändern", "Cambiar portada", "Changer la pochette", "更换封面"]),
    ("Remove cover", ["Kapağı kaldır", "Cover entfernen", "Quitar portada", "Retirer la pochette", "移除封面"]),
    ("Export", ["Dışa aktar", "Exportieren", "Exportar", "Exporter", "导出"]),
    ("Import playlist", ["Çalma listesini içe aktar", "Playlist importieren", "Importar lista", "Importer une playlist", "导入播放列表"]),
    ("Restart required to apply your changes.", ["Değişikliklerin uygulanması için yeniden başlatma gerekli.", "Neustart erforderlich, um die Änderungen anzuwenden.", "Se requiere reiniciar para aplicar los cambios.", "Un redémarrage est nécessaire pour appliquer les changements.", "需要重启才能应用更改。"]),
    ("Theme", ["Tema", "Design", "Tema", "Thème", "主题"]),
    ("Appearance", ["Görünüm", "Darstellung", "Apariencia", "Apparence", "外观"]),
    ("Dark", ["Koyu", "Dunkel", "Oscuro", "Sombre", "深色"]),
    ("Light", ["Açık", "Hell", "Claro", "Clair", "浅色"]),
    ("Content", ["İçerik", "Inhalte", "Contenido", "Contenu", "内容"]),
    ("Refresh Home and Explore", ["Ana sayfayı ve Keşfet'i yenile", "Start und Entdecken aktualisieren", "Actualizar Inicio y Explorar", "Actualiser Accueil et Explorer", "刷新首页和探索"]),
    ("Never", ["Asla", "Nie", "Nunca", "Jamais", "从不"]),
    ("minutes", ["dakika", "Minuten", "minutos", "minutes", "分钟"]),
    ("hours", ["saat", "Stunden", "horas", "heures", "小时"]),
    ("A page you open again after this long is fetched again.", ["Bu süreden sonra yeniden açtığın sayfa tekrar yüklenir.", "Eine Seite, die du nach dieser Zeit erneut öffnest, wird neu geladen.", "Una página que abras de nuevo pasado este tiempo se vuelve a cargar.", "Une page rouverte après ce délai est rechargée.", "超过此时间后再次打开的页面将重新加载。"]),
    ("Application data", ["Uygulama verileri", "Anwendungsdaten", "Datos de la aplicación", "Données de l’application", "应用数据"]),
    ("Data folder", ["Veri klasörü", "Datenordner", "Carpeta de datos", "Dossier de données", "数据文件夹"]),
    ("(unknown)", ["(bilinmiyor)", "(unbekannt)", "(desconocido)", "(inconnu)", "（未知）"]),
    ("Open folder", ["Klasörü aç", "Ordner öffnen", "Abrir carpeta", "Ouvrir le dossier", "打开文件夹"]),
    ("Change folder…", ["Klasörü değiştir…", "Ordner ändern…", "Cambiar carpeta…", "Changer de dossier…", "更改文件夹…"]),
    ("Your settings, library, playlists and covers are stored in this folder. Changing it copies everything to the new folder; the old folder is left in place.", ["Ayarların, kitaplığın, çalma listelerin ve kapaklar bu klasörde saklanır. Değiştirmek her şeyi yeni klasöre kopyalar; eski klasör yerinde kalır.", "Deine Einstellungen, Mediathek, Playlists und Cover liegen in diesem Ordner. Beim Ändern wird alles in den neuen Ordner kopiert; der alte Ordner bleibt bestehen.", "Tus ajustes, biblioteca, listas y portadas se guardan en esta carpeta. Al cambiarla se copia todo a la nueva carpeta; la anterior se deja donde está.", "Vos réglages, bibliothèque, playlists et pochettes sont stockés dans ce dossier. Le changer copie tout vers le nouveau dossier ; l’ancien reste en place.", "你的设置、媒体库、播放列表和封面都存储在此文件夹中。更改后会将所有内容复制到新文件夹；旧文件夹保持不变。"]),
    ("Back up data", ["Verileri yedekle", "Daten sichern", "Copia de seguridad de datos", "Sauvegarder les données", "备份数据"]),
    ("Restore from backup", ["Yedekten geri yükle", "Aus Sicherung wiederherstellen", "Restaurar desde copia", "Restaurer une sauvegarde", "从备份恢复"]),
    ("A backup holds all playlists, liked songs, saved items and your settings (theme, language, refresh interval).", ["Yedek tüm çalma listelerini, beğenilenleri, kayıtlı öğeleri ve ayarlarını (tema, dil, yenileme aralığı) içerir.", "Eine Sicherung enthält alle Playlists, gemochten Songs, gespeicherten Elemente und deine Einstellungen (Design, Sprache, Aktualisierungsintervall).", "Una copia incluye todas las listas, canciones que te gustan, elementos guardados y tus ajustes (tema, idioma, intervalo de actualización).", "Une sauvegarde contient toutes les playlists, titres aimés, éléments enregistrés et vos réglages (thème, langue, intervalle d’actualisation).", "备份包含所有播放列表、喜欢的歌曲、已保存的项目和你的设置（主题、语言、刷新间隔）。"]),
    ("Delete this playlist?", ["Bu çalma listesi silinsin mi?", "Diese Playlist löschen?", "¿Eliminar esta lista?", "Supprimer cette playlist ?", "删除此播放列表？"]),
    ("This cannot be undone.", ["Bu geri alınamaz.", "Das kann nicht rückgängig gemacht werden.", "Esto no se puede deshacer.", "Cette action est irréversible.", "此操作无法撤销。"]),
    ("Delete", ["Sil", "Löschen", "Eliminar", "Supprimer", "删除"]),
    ("songs", ["şarkı", "Songs", "canciones", "titres", "首歌曲"]),
    ("playlists", ["çalma listesi", "Playlists", "listas", "playlists", "个播放列表"]),
    ("liked songs", ["beğenilen şarkı", "gemochte Songs", "canciones que te gustan", "titres aimés", "首喜欢的歌曲"]),
    ("saved items", ["kayıtlı öğe", "gespeicherte Elemente", "elementos guardados", "éléments enregistrés", "个已保存项目"]),
    ("Replace your data?", ["Verilerin değiştirilsin mi?", "Daten ersetzen?", "¿Reemplazar tus datos?", "Remplacer vos données ?", "替换你的数据？"]),
    ("Your current playlists, liked songs and saved items will be replaced by the backup. If it has settings, they are restored too.", ["Mevcut çalma listelerin, beğenilerin ve kayıtlı öğelerin yedekle değiştirilecek. Yedekte ayarlar varsa onlar da geri yüklenir.", "Deine Playlists, gemochten Songs und gespeicherten Elemente werden durch die Sicherung ersetzt. Enthält sie Einstellungen, werden auch diese wiederhergestellt.", "Tus listas, canciones que te gustan y elementos guardados se reemplazarán por la copia. Si incluye ajustes, también se restauran.", "Vos playlists, titres aimés et éléments enregistrés seront remplacés par la sauvegarde. Si elle contient des réglages, ils sont aussi restaurés.", "你当前的播放列表、喜欢的歌曲和已保存的项目将被备份替换。如果备份包含设置，也会一并恢复。"]),
    ("Backup contains", ["Yedekte var", "Die Sicherung enthält", "La copia contiene", "La sauvegarde contient", "备份包含"]),
    ("Rename", ["Yeniden adlandır", "Umbenennen", "Renombrar", "Renommer", "重命名"]),
    ("Delete playlist", ["Çalma listesini sil", "Playlist löschen", "Eliminar lista", "Supprimer la playlist", "删除播放列表"]),
    ("Remove from library", ["Kitaplıktan kaldır", "Aus Mediathek entfernen", "Quitar de la biblioteca", "Retirer de la bibliothèque", "从媒体库中移除"]),
    ("No liked songs yet", ["Henüz beğenilen şarkı yok", "Noch keine gemochten Songs", "Aún no hay canciones que te gusten", "Aucun titre aimé pour l’instant", "还没有喜欢的歌曲"]),
    ("Tap the heart on a song to keep it here.", ["Bir şarkıdaki kalbe dokun, burada dursun.", "Tippe auf das Herz, um einen Song hier zu speichern.", "Pulsa el corazón de una canción para guardarla aquí.", "Touchez le cœur d’un titre pour le garder ici.", "点按歌曲上的爱心即可保存在这里。"]),
    ("No saved albums", ["Kayıtlı albüm yok", "Keine gespeicherten Alben", "No hay álbumes guardados", "Aucun album enregistré", "没有已保存的专辑"]),
    ("No saved artists", ["Kayıtlı sanatçı yok", "Keine gespeicherten Künstler", "No hay artistas guardados", "Aucun artiste enregistré", "没有已保存的艺人"]),
    ("New playlist", ["Yeni çalma listesi", "Neue Playlist", "Nueva lista", "Nouvelle playlist", "新建播放列表"]),
    ("New playlist…", ["Yeni çalma listesi…", "Neue Playlist…", "Nueva lista…", "Nouvelle playlist…", "新建播放列表…"]),
    ("Rename playlist", ["Çalma listesini yeniden adlandır", "Playlist umbenennen", "Renombrar lista", "Renommer la playlist", "重命名播放列表"]),
    ("No playlists yet", ["Henüz çalma listesi yok", "Noch keine Playlists", "Aún no hay listas", "Aucune playlist pour l’instant", "还没有播放列表"]),
    ("Save", ["Kaydet", "Speichern", "Guardar", "Enregistrer", "保存"]),
    ("Saved", ["Kaydedildi", "Gespeichert", "Guardado", "Enregistré", "已保存"]),
    ("Create", ["Oluştur", "Erstellen", "Crear", "Créer", "创建"]),
    ("Cancel", ["İptal", "Abbrechen", "Cancelar", "Annuler", "取消"]),
    ("Done", ["Bitti", "Fertig", "Hecho", "Terminé", "完成"]),
    ("Edit", ["Düzenle", "Bearbeiten", "Editar", "Modifier", "编辑"]),
    ("Previous", ["Önceki", "Zurück", "Anterior", "Précédent", "上一首"]),
    ("Next", ["Sonraki", "Weiter", "Siguiente", "Suivant", "下一首"]),
    ("Repeat one", ["Birini tekrarla", "Titel wiederholen", "Repetir una", "Répéter un titre", "单曲循环"]),
    ("Repeat all", ["Tümünü tekrarla", "Alle wiederholen", "Repetir todo", "Tout répéter", "列表循环"]),
    ("Repeat off", ["Tekrar kapalı", "Wiederholung aus", "Repetición desactivada", "Répétition désactivée", "不循环"]),
    ("Mute", ["Sessiz", "Stumm", "Silenciar", "Muet", "静音"]),
    ("Open now playing", ["Çalanı aç", "Wiedergabe öffnen", "Abrir reproducción", "Ouvrir la lecture", "打开正在播放"]),
    ("Close now playing", ["Çalanı kapat", "Wiedergabe schließen", "Cerrar reproducción", "Fermer la lecture", "关闭正在播放"]),
    ("Play next", ["Sıradaki olarak oynat", "Als Nächstes spielen", "Reproducir siguiente", "Lire ensuite", "下一首播放"]),
    ("Start radio", ["Radyo başlat", "Radio starten", "Iniciar radio", "Lancer la radio", "开启电台"]),
    ("Add to playlist", ["Çalma listesine ekle", "Zu Playlist hinzufügen", "Añadir a lista", "Ajouter à une playlist", "添加到播放列表"]),
    ("Go to artist", ["Sanatçıya git", "Zum Künstler", "Ir al artista", "Aller à l’artiste", "前往艺人"]),
    ("Go to album", ["Albüme git", "Zum Album", "Ir al álbum", "Aller à l’album", "前往专辑"]),
    ("More actions", ["Diğer işlemler", "Weitere Aktionen", "Más acciones", "Plus d’actions", "更多操作"]),
    ("Remove from playlist", ["Çalma listesinden kaldır", "Aus Playlist entfernen", "Quitar de la lista", "Retirer de la playlist", "从播放列表中移除"]),
    ("Remove from this playlist", ["Bu çalma listesinden kaldır", "Aus dieser Playlist entfernen", "Quitar de esta lista", "Retirer de cette playlist", "从此播放列表中移除"]),
    ("Move up", ["Yukarı taşı", "Nach oben", "Subir", "Monter", "上移"]),
    ("Move down", ["Aşağı taşı", "Nach unten", "Bajar", "Descendre", "下移"]),
    ("Playlist", ["Çalma listesi", "Playlist", "Lista", "Playlist", "播放列表"]),
    ("Playlist on this device", ["Bu cihazdaki çalma listesi", "Playlist auf diesem Gerät", "Lista en este dispositivo", "Playlist sur cet appareil", "此设备上的播放列表"]),
    ("Playlist not found", ["Çalma listesi bulunamadı", "Playlist nicht gefunden", "Lista no encontrada", "Playlist introuvable", "未找到播放列表"]),
    ("It may have been deleted.", ["Silinmiş olabilir.", "Sie wurde möglicherweise gelöscht.", "Puede que se haya eliminado.", "Elle a peut-être été supprimée.", "它可能已被删除。"]),
    ("Back to playlists", ["Çalma listelerine dön", "Zurück zu Playlists", "Volver a las listas", "Retour aux playlists", "返回播放列表"]),
    ("Nothing to edit yet.", ["Düzenlenecek bir şey yok.", "Noch nichts zu bearbeiten.", "Aún no hay nada que editar.", "Rien à modifier pour l’instant.", "暂无可编辑的内容。"]),
    ("Country", ["Ülke", "Land", "País", "Pays", "国家/地区"]),
    ("Minimize", ["Küçült", "Minimieren", "Minimizar", "Réduire", "最小化"]),
    ("Maximize", ["Büyüt", "Maximieren", "Maximizar", "Agrandir", "最大化"]),
    ("Restore", ["Geri yükle", "Wiederherstellen", "Restaurar", "Restaurer", "恢复"]),
    ("Keyboard shortcuts", ["Klavye kısayolları", "Tastenkürzel", "Atajos de teclado", "Raccourcis clavier", "键盘快捷键"]),
    ("Playback", ["Oynatma", "Wiedergabe", "Reproducción", "Lecture", "播放"]),
    ("Navigation", ["Gezinme", "Navigation", "Navegación", "Navigation", "导航"]),
    ("Play / pause", ["Oynat / duraklat", "Wiedergabe / Pause", "Reproducir / pausar", "Lecture / pause", "播放 / 暂停"]),
    ("Next track", ["Sonraki parça", "Nächster Titel", "Pista siguiente", "Titre suivant", "下一首"]),
    ("Previous track", ["Önceki parça", "Vorheriger Titel", "Pista anterior", "Titre précédent", "上一首"]),
    ("Seek 5 seconds", ["5 saniye ileri / geri", "5 Sekunden vor / zurück", "Avanzar / retroceder 5 s", "Avancer / reculer de 5 s", "快进 / 快退 5 秒"]),
    ("Volume up / down", ["Ses artır / azalt", "Lauter / leiser", "Subir / bajar volumen", "Volume + / −", "音量增 / 减"]),
    ("Repeat", ["Tekrarla", "Wiederholen", "Repetir", "Répéter", "循环"]),
    ("Like the current song", ["Çalan şarkıyı beğen", "Aktuellen Titel liken", "Me gusta la canción actual", "Aimer le titre en cours", "喜欢当前歌曲"]),
    ("Search", ["Ara", "Suchen", "Buscar", "Rechercher", "搜索"]),
    ("Home, Explore, Library, Playlists", ["Ana sayfa, Keşfet, Kitaplık, Listeler", "Start, Entdecken, Mediathek, Playlists", "Inicio, Explorar, Biblioteca, Listas", "Accueil, Explorer, Bibliothèque, Playlists", "主页、探索、音乐库、播放列表"]),
    ("Desktop", ["Masaüstü", "Desktop", "Escritorio", "Bureau", "桌面"]),
    ("Show tray icon", ["Sistem tepsisi simgesini göster", "Tray-Symbol anzeigen", "Mostrar icono de bandeja", "Afficher l’icône de la zone de notification", "显示托盘图标"]),
    ("Closing the window keeps Tunebox running in the tray", ["Pencereyi kapatınca Tunebox tepside çalışmaya devam eder", "Beim Schließen läuft Tunebox im Tray weiter", "Al cerrar la ventana, Tunebox sigue en la bandeja", "Fermer la fenêtre laisse Tunebox actif dans la zone de notification", "关闭窗口后 Tunebox 继续在托盘中运行"]),
    ("Show the song next to the tray icon", ["Şarkıyı tepsi simgesinin yanında göster", "Titel neben dem Tray-Symbol anzeigen", "Mostrar la canción junto al icono de bandeja", "Afficher le titre à côté de l’icône", "在托盘图标旁显示歌曲"]),
    ("Needs a tray host that shows labels, e.g. GNOME's AppIndicator extension.", ["Etiket gösteren bir tepsi gerekir, örn. GNOME AppIndicator eklentisi.", "Benötigt ein Tray, das Beschriftungen zeigt, z. B. die GNOME-AppIndicator-Erweiterung.", "Requiere una bandeja que muestre etiquetas, p. ej. la extensión AppIndicator de GNOME.", "Nécessite une zone de notification affichant les libellés, p. ex. l’extension AppIndicator de GNOME.", "需要支持文字标签的托盘，例如 GNOME 的 AppIndicator 扩展。"]),
    ("Continue where you left off", ["Kaldığın yerden devam et", "Dort weitermachen, wo du aufgehört hast", "Continuar donde lo dejaste", "Reprendre là où vous vous êtes arrêté", "从上次停下的地方继续"]),
    ("Restores the queue, song and position (paused) when Tunebox starts.", ["Tunebox açılınca sırayı, şarkıyı ve konumu (duraklatılmış) geri yükler.", "Stellt beim Start Warteschlange, Titel und Position (pausiert) wieder her.", "Al iniciar, restaura la cola, la canción y la posición (en pausa).", "Au démarrage, restaure la file, le titre et la position (en pause).", "启动时恢复队列、歌曲和播放位置（已暂停）。"]),
    ("Notify when the song changes", ["Şarkı değişince bildir", "Bei Titelwechsel benachrichtigen", "Avisar al cambiar de canción", "Notifier à chaque changement de titre", "切换歌曲时通知"]),
    ("Only shown while Tunebox is in the background.", ["Yalnızca Tunebox arka plandayken gösterilir.", "Nur, wenn Tunebox im Hintergrund ist.", "Solo si Tunebox está en segundo plano.", "Uniquement quand Tunebox est en arrière-plan.", "仅在 Tunebox 位于后台时显示。"]),
    ("Show Tunebox", ["Tunebox'ı göster", "Tunebox anzeigen", "Mostrar Tunebox", "Afficher Tunebox", "显示 Tunebox"]),
    ("On", ["Açık", "An", "Activado", "Activé", "开"]),
    ("Off", ["Kapalı", "Aus", "Desactivado", "Désactivé", "关"]),
    ("Applies the next time Tunebox starts.", ["Tunebox'ın bir sonraki açılışında geçerli olur.", "Gilt ab dem nächsten Start von Tunebox.", "Se aplica la próxima vez que se inicie Tunebox.", "S’applique au prochain démarrage de Tunebox.", "下次启动 Tunebox 时生效。"]),
    ("Only works while the tray icon is showing.", ["Yalnızca tepsi simgesi görünürken çalışır.", "Funktioniert nur, solange das Tray-Symbol angezeigt wird.", "Solo funciona mientras se muestre el icono de la bandeja.", "Ne fonctionne que si l’icône est affichée.", "仅在托盘图标显示时有效。"]),
    ("Quit", ["Çık", "Beenden", "Salir", "Quitter", "退出"]),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_is_identity_and_unknown_falls_back() {
        set(Lang::En);
        assert_eq!(t("Play"), "Play");
        set(Lang::Tr);
        assert_eq!(t("Play"), "Oynat");
        assert_eq!(
            t("Some string nobody translated"),
            "Some string nobody translated"
        );
        set(Lang::En);
    }

    #[test]
    fn table_has_no_duplicates_or_blanks() {
        let mut seen = std::collections::HashSet::new();
        for (k, v) in TABLE {
            assert!(seen.insert(*k), "duplicate key {k}");
            assert!(v.iter().all(|s| !s.is_empty()), "blank translation for {k}");
        }
    }

    #[test]
    fn codes_round_trip() {
        for l in Lang::ALL {
            assert_eq!(Lang::from_code(l.code()), l);
        }
        assert_eq!(Lang::from_code("tr-TR"), Lang::Tr);
        assert_eq!(Lang::from_code("zh-CN"), Lang::Zh);
        assert_eq!(Lang::Zh.hl(), "zh-CN");
        assert_eq!(Lang::Fr.hl(), "fr");
        assert_eq!(Lang::from_code("xx"), Lang::En);
    }
}
