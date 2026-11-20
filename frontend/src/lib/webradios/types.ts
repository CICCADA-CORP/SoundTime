export interface WebradioStation {
  id: string;
  name: string;
  icon_url: string | null;
  description: string | null;
  tags: string[];
  country: string | null;
  language: string | null;
  codec: string | null;
  bitrate: number | null;
  hls: boolean;
}

export interface StationPage {
  stations: WebradioStation[];
  next_offset: number | null;
}

export interface WebradioStatus {
  instance_enabled: boolean;
  user_enabled: boolean;
  enabled: boolean;
}

export function stationUrl(value: string | null, allowHttp = false) {
  try {
    const url = new URL(value ?? "");
    return (url.protocol === "https:" || (allowHttp && url.protocol === "http:")) && !url.username && !url.password ? url.href : null;
  } catch { return null; }
}
