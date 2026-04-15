export type HealthResponse = {
  status: string;
  version: string;
};

export type PathsResponse = {
  config_file_path: string;
  config_file_exists: boolean;
  devices_file_path: string;
  devices_file_exists: boolean;
};

export type VisualizerConfig = {
  audio_backend: string | null;
  freq_range: [number, number] | null;
  colors: Array<[number, number, number]> | null;
  default_gain: number | null;
  transition_time: number | null;
  time_window: number | null;
  primary_axis: string | null;
  sort_primary: string | null;
  sort_secondary: string | null;
  effect: string | null;
};

export type ConfigPayload = {
  default_nl_device_name: string | null;
  visualizer_config: VisualizerConfig;
};

export type ConfigResponse = {
  paths: PathsResponse;
  config: ConfigPayload | null;
};

export type VisualizerSortUpdateRequest = {
  primary_axis: "X" | "Y";
  sort_primary: "Asc" | "Desc";
  sort_secondary: "Asc" | "Desc";
};

export type VisualizerSettingsUpdateRequest = {
  audio_backend?: string;
  freq_range?: [number, number];
  default_gain?: number;
  transition_time?: number;
  time_window?: number;
};

export type NowPlayingTrack = {
  title: string | null;
  artist: string | null;
  album: string | null;
  stream_url: string | null;
  source_name: string | null;
  source_ip: string | null;
  user_agent: string | null;
};

export type NowPlayingResponse = {
  reader_running: boolean;
  metadata_pipe_path: string;
  last_error: string | null;
  drive_visualizer_palette: boolean;
  track: NowPlayingTrack | null;
  palette_colors: Array<[number, number, number]>;
  artwork_available: boolean;
  artwork_generation: number;
  updated_at_ms: number | null;
};

export type NowPlayingSettingsUpdateRequest = {
  drive_visualizer_palette?: boolean;
};

export type DeviceSummary = {
  name: string;
  ip: string;
};

export type DevicesResponse = {
  devices: DeviceSummary[];
  devices_file_path: string;
  devices_file_exists: boolean;
};

export type DeviceInfoResponse = {
  device: DeviceSummary;
  info: Record<string, unknown>;
};

export type DeviceLayoutPanel = {
  panel_id: number;
  x: number;
  y: number;
  orientation: number;
  shape_type_id: number;
  shape_type_name: string;
  num_sides: number;
  side_length: number;
};

export type DeviceLayoutResponse = {
  device: DeviceSummary;
  global_orientation: number;
  panels: DeviceLayoutPanel[];
};

export type DeviceStateUpdateRequest = {
  power_on?: boolean;
  brightness?: number;
};

export type DeviceStateUpdateResponse = {
  device: DeviceSummary;
  power_on: boolean | null;
  brightness: number | null;
};

export type PaletteEntry = {
  name: string;
  colors: Array<[number, number, number]>;
};

export type PalettesResponse = {
  palettes: PaletteEntry[];
};

export type AudioBackendsResponse = {
  current_audio_backend: string | null;
  available_audio_backends: string[];
};

export type VisualizerPreviewPanelColor = {
  panel_id: number;
  rgb: [number, number, number];
};

export type VisualizerPreviewResponse = {
  enabled: boolean;
  device: DeviceSummary | null;
  panel_colors: VisualizerPreviewPanelColor[];
};

export type VisualizerStatusResponse = {
  status: string;
  stream_health: string;
  live_visualizer_attached: boolean;
  restart_cooldown_active: boolean;
  consecutive_restart_failures: number;
  healthy_ping_streak: number;
  auto_fallback_to_default_active: boolean;
  current_audio_backend: string | null;
  device: DeviceSummary | null;
};

async function apiGet<T>(path: string): Promise<T> {
  const response = await fetch(path);
  return parseResponse<T>(response);
}

async function apiSend<T>(path: string, init: RequestInit): Promise<T> {
  const response = await fetch(path, init);
  return parseResponse<T>(response);
}

async function parseResponse<T>(response: Response): Promise<T> {
  if (!response.ok) {
    let errorMessage = `${response.status} ${response.statusText}`;
    try {
      const parsed = (await response.json()) as { error?: string };
      if (parsed.error) {
        errorMessage = parsed.error;
      }
    } catch {
      // Keep fallback status text if body is not JSON.
    }
    throw new Error(errorMessage);
  }

  return (await response.json()) as T;
}

export const api = {
  health: () => apiGet<HealthResponse>("/api/health"),
  config: () => apiGet<ConfigResponse>("/api/config"),
  saveConfig: () =>
    apiSend<ConfigResponse>("/api/config/save", {
      method: "POST",
    }),
  setVisualizerEffect: (effect: string) =>
    apiSend<ConfigResponse>("/api/config/visualizer/effect", {
      method: "PUT",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify({ effect }),
    }),
  setVisualizerPalette: (palette_name: string) =>
    apiSend<ConfigResponse>("/api/config/visualizer/palette", {
      method: "PUT",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify({ palette_name }),
    }),
  setVisualizerSort: (payload: VisualizerSortUpdateRequest) =>
    apiSend<ConfigResponse>("/api/config/visualizer/sort", {
      method: "PUT",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify(payload),
    }),
  setVisualizerSettings: (payload: VisualizerSettingsUpdateRequest) =>
    apiSend<ConfigResponse>("/api/config/visualizer/settings", {
      method: "PUT",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify(payload),
    }),
  nowPlaying: () => apiGet<NowPlayingResponse>("/api/now-playing"),
  setNowPlayingSettings: (payload: NowPlayingSettingsUpdateRequest) =>
    apiSend<NowPlayingResponse>("/api/now-playing/settings", {
      method: "PUT",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify(payload),
    }),
  visualizerPreview: () =>
    apiGet<VisualizerPreviewResponse>("/api/visualizer/preview"),
  visualizerStatus: () =>
    apiGet<VisualizerStatusResponse>("/api/visualizer/status"),
  audioBackends: () => apiGet<AudioBackendsResponse>("/api/audio/backends"),
  devices: () => apiGet<DevicesResponse>("/api/devices"),
  deviceInfo: (name: string) =>
    apiGet<DeviceInfoResponse>(`/api/devices/${encodeURIComponent(name)}/info`),
  deviceLayout: (name: string) =>
    apiGet<DeviceLayoutResponse>(`/api/devices/${encodeURIComponent(name)}/layout`),
  setDeviceState: (name: string, payload: DeviceStateUpdateRequest) =>
    apiSend<DeviceStateUpdateResponse>(
      `/api/devices/${encodeURIComponent(name)}/state`,
      {
        method: "PUT",
        headers: {
          "Content-Type": "application/json",
        },
        body: JSON.stringify(payload),
      },
    ),
  palettes: () => apiGet<PalettesResponse>("/api/palettes"),
};
