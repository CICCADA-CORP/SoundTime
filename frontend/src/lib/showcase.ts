import type { Track } from "$lib/types";

// These four original instrumentals ship with the client for the offline preview.
const recordings = [
  { slug: "last-light", title: "Last Light", genre: "Downtempo", seconds: 22, bytes: 199006 },
  { slug: "blue-fold", title: "Blue Fold", genre: "Electronica", seconds: 19, bytes: 174031 },
  { slug: "slow-bloom", title: "Slow Bloom", genre: "Ambient", seconds: 26, bytes: 227990 },
  { slug: "dune-line", title: "Dune Line", genre: "Nu jazz", seconds: 21, bytes: 193958 },
];

export const showcaseTracks: Track[] = recordings.map(({ slug, title, genre, seconds, bytes }, i) => ({
  id: `showcase-${slug}`, title, genre, artist_name: "SoundTime Studio", artist_id: "",
  album_title: "After Hours", album_id: null, cover_url: `/art/${slug}.webp`,
  file_path: `/audio/${slug}.m4a`, duration_secs: seconds, file_size: bytes,
  track_number: i + 1, disc_number: null, year: null, format: "AAC", bitrate: 80,
  sample_rate: 24000, musicbrainz_id: null, waveform_data: null,
  uploaded_by: null, play_count: 0, created_at: "",
}));

export function showcaseAudio(id: string) {
  return showcaseTracks.find((track) => track.id === id)?.file_path;
}
