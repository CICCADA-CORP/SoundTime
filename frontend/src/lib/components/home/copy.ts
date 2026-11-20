import { getLocale, type Locale } from "$lib/i18n/index.svelte";

const en = {
  cover: "Cover of {title}",
  title: "Listen Now",
  demo: "Demo mode",
  retry: "Retry",
  offline: "The catalogue is unavailable. Retry connection",
  incomplete: "Some sections couldn’t load.",
  rotation: "In rotation",
  play: "Listen now",
  pause: "Pause",
  tracklist: "Tracklist",
  grid: "Cover view",
  list: "List view",
  empty: "Add your first tracks and start building your music collection.",

  topTracks: "Most played", allTime: "All time", added: "Added to queue",
};

const copy: Record<Locale, typeof en> = {
  en,
  fr: {
    cover: "Pochette de {title}",

    topTracks: "Les plus écoutés", allTime: "Depuis toujours", added: "Ajouté à la file",
    title: "À écouter", demo: "Mode démo", retry: "Réessayer",
    offline: "Le catalogue est indisponible. Réessayer la connexion",
    incomplete: "Certaines rubriques sont indisponibles.",
    rotation: "En rotation",
    play: "Écouter", pause: "Pause", tracklist: "Voir les titres",
    grid: "Vue pochettes", list: "Vue liste",
    empty: "Ajoutez vos premiers morceaux pour commencer votre collection.",
  },
  es: {
    cover: "Portada de {title}",

    topTracks: "Lo más escuchado", allTime: "Todo el tiempo", added: "Añadido a la cola",
    title: "Escuchar ahora", demo: "Modo demo", retry: "Reintentar",
    offline: "El catálogo no está disponible. Reintentar la conexión",
    incomplete: "No se pudieron cargar algunas secciones.",
    rotation: "En rotación",
    play: "Escuchar", pause: "Pausar", tracklist: "Ver canciones",
    grid: "Ver portadas", list: "Ver lista",
    empty: "Añade tus primeras canciones para empezar tu colección.",
  },
  zh: {
    cover: "《{title}》封面",

    topTracks: "播放最多", allTime: "全部时间", added: "已加入队列",
    title: "现在就听", demo: "演示模式", retry: "重试",
    offline: "曲库暂时无法连接。重试连接", incomplete: "部分内容加载失败。",
    rotation: "轮播精选",
    play: "立即播放", pause: "暂停",
    tracklist: "查看曲目", grid: "封面视图", list: "列表视图",
    empty: "添加第一首歌曲，开始建立你的音乐收藏。",
  },
  ru: {
    cover: "Обложка «{title}»",

    topTracks: "Самые прослушиваемые", allTime: "За всё время", added: "Добавлено в очередь",
    title: "Слушать сейчас", demo: "Деморежим", retry: "Повторить",
    offline: "Каталог недоступен. Повторить подключение",
    incomplete: "Не удалось загрузить часть разделов.",
    rotation: "В ротации",
    play: "Слушать", pause: "Пауза", tracklist: "Список треков",
    grid: "Обложки", list: "Список",
    empty: "Добавьте первые треки, чтобы начать собирать коллекцию.",
  },
};

export const homeCopy = () => copy[getLocale()];
