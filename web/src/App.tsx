import { useEffect, useRef, useState, type ReactNode } from "react";
import {
  api,
  apiAssetUrl,
  type AudioBackendsResponse,
  type ConfigResponse,
  type DeviceInfoResponse,
  type DeviceLayoutPanel,
  type DeviceLayoutResponse,
  type DevicesResponse,
  type DeviceStateUpdateRequest,
  type NowPlayingResponse,
  type VisualizerSettingsUpdateRequest,
  type VisualizerStatusResponse,
  type VisualizerSortUpdateRequest,
  type HealthResponse,
  type PaletteEntry,
  type PalettesResponse,
} from "@/api";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Separator } from "@/components/ui/separator";
import { Switch } from "@/components/ui/switch";

type LoadState = "idle" | "loading" | "ready" | "error";
const DEFAULT_BRIGHTNESS_DRAFT = "50";
const EFFECT_OPTIONS = ["Spectrum", "EnergyWave", "Pulse"] as const;
type EffectOption = (typeof EFFECT_OPTIONS)[number];

function isValidBrightnessInput(value: string): boolean {
  return /^\d{0,3}$/.test(value);
}

function parseBrightness(value: string): number | null {
  if (value.trim() === "") {
    return null;
  }
  const parsed = Number(value);
  if (!Number.isInteger(parsed) || parsed < 0 || parsed > 100) {
    return null;
  }
  return parsed;
}

function parseInteger(value: string): number | null {
  const trimmed = value.trim();
  if (trimmed === "") {
    return null;
  }
  const parsed = Number(trimmed);
  if (!Number.isInteger(parsed)) {
    return null;
  }
  return parsed;
}

function parseNonNegativeFloat(value: string): number | null {
  const trimmed = value.trim();
  if (trimmed === "") {
    return null;
  }
  const parsed = Number(trimmed);
  if (!Number.isFinite(parsed) || parsed < 0) {
    return null;
  }
  return parsed;
}

function parsePositiveFloat(value: string): number | null {
  const trimmed = value.trim();
  if (trimmed === "") {
    return null;
  }
  const parsed = Number(trimmed);
  if (!Number.isFinite(parsed) || parsed <= 0) {
    return null;
  }
  return parsed;
}

function formatTenths(value: number): string {
  return (Math.round(value * 10) / 10).toFixed(1);
}

function extractBrightnessFromInfo(info: Record<string, unknown>): number | null {
  const state = info.state;
  if (!state || typeof state !== "object") {
    return null;
  }
  const brightness = (state as Record<string, unknown>).brightness;
  if (!brightness || typeof brightness !== "object") {
    return null;
  }
  const value = (brightness as Record<string, unknown>).value;
  const parsed = Number(value);
  if (!Number.isInteger(parsed) || parsed < 0 || parsed > 100) {
    return null;
  }
  return parsed;
}

function normalizeEffect(value: string | null | undefined): EffectOption {
  if (value === "EnergyWave") {
    return "EnergyWave";
  }
  if (value === "Pulse") {
    return "Pulse";
  }
  return "Spectrum";
}

function inferPaletteName(
  colors: Array<[number, number, number]> | null | undefined,
  palettes: PaletteEntry[],
): string | null {
  if (!colors?.length) {
    return null;
  }

  for (const palette of palettes) {
    if (palette.colors.length !== colors.length) {
      continue;
    }
    const allEqual = palette.colors.every((color, index) => {
      const current = colors[index];
      return (
        color[0] === current[0] && color[1] === current[1] && color[2] === current[2]
      );
    });
    if (allEqual) {
      return palette.name;
    }
  }
  return null;
}

function resolveInitialLayoutDeviceName(
  nextConfig: ConfigResponse,
  nextDevices: DevicesResponse,
): string | null {
  const configured = nextConfig.config?.default_nl_device_name;
  if (configured && nextDevices.devices.some((device) => device.name === configured)) {
    return configured;
  }
  return nextDevices.devices[0]?.name ?? null;
}

function App() {
  const [loadState, setLoadState] = useState<LoadState>("idle");
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [actionMessage, setActionMessage] = useState<string | null>(null);
  const [updatingDeviceName, setUpdatingDeviceName] = useState<string | null>(null);
  const [loadingDeviceName, setLoadingDeviceName] = useState<string | null>(null);
  const [savingConfigSection, setSavingConfigSection] = useState<
    "effect" | "palette" | "sort" | "settings" | "persist" | null
  >(null);
  const [brightnessDraftByDevice, setBrightnessDraftByDevice] = useState<
    Record<string, string>
  >({});
  const [effectDraft, setEffectDraft] = useState<EffectOption>("Spectrum");
  const [paletteDraft, setPaletteDraft] = useState<string>("");
  const [sortDraft, setSortDraft] = useState<VisualizerSortUpdateRequest>({
    primary_axis: "Y",
    sort_primary: "Asc",
    sort_secondary: "Asc",
  });
  const [settingsDraft, setSettingsDraft] = useState({
    audio_backend: "default",
    freq_min: "20",
    freq_max: "4500",
    default_gain: "1",
    transition_time: "0.2",
    time_window: "0.2",
  });
  const [audioBackends, setAudioBackends] = useState<string[]>(["default"]);
  const [showLivePreview, setShowLivePreview] = useState(false);
  const [livePreviewDeviceName, setLivePreviewDeviceName] = useState<string | null>(null);
  const [livePreviewColorsByPanel, setLivePreviewColorsByPanel] = useState<
    Record<number, [number, number, number]>
  >({});
  const [visualizerStatus, setVisualizerStatus] = useState<VisualizerStatusResponse | null>(
    null,
  );
  const [nowPlaying, setNowPlaying] = useState<NowPlayingResponse | null>(null);
  const brightnessCommitTimersRef = useRef<Record<string, number>>({});
  const lastAppliedBrightnessRef = useRef<Record<string, number>>({});

  const [health, setHealth] = useState<HealthResponse | null>(null);
  const [config, setConfig] = useState<ConfigResponse | null>(null);
  const [devices, setDevices] = useState<DevicesResponse | null>(null);
  const [palettes, setPalettes] = useState<PalettesResponse | null>(null);
  const [selectedDeviceInfo, setSelectedDeviceInfo] =
    useState<DeviceInfoResponse | null>(null);
  const [selectedDeviceLayout, setSelectedDeviceLayout] =
    useState<DeviceLayoutResponse | null>(null);

  const visualizerConfig = config?.config?.visualizer_config;
  const availableBackendOptions = Array.from(
    new Set([settingsDraft.audio_backend, ...audioBackends].filter((name) => name.trim().length)),
  );

  useEffect(() => {
    let isMounted = true;

    async function loadData() {
      try {
        setLoadState("loading");
        setErrorMessage(null);

        const [healthData, configData, devicesData, palettesData] = await Promise.all([
          api.health(),
          api.config(),
          api.devices(),
          api.palettes(),
        ]);
        let nowPlayingData: NowPlayingResponse | null = null;
        try {
          nowPlayingData = await api.nowPlaying();
        } catch {
          // Keep the dashboard usable if now-playing metadata is unavailable.
        }
        const brightnessEntries = await Promise.all(
          devicesData.devices.map(async (device) => {
            try {
              const info = await api.deviceInfo(device.name);
              const currentBrightness = extractBrightnessFromInfo(info.info);
              return [
                device.name,
                String(currentBrightness ?? Number(DEFAULT_BRIGHTNESS_DRAFT)),
              ] as const;
            } catch {
              return [device.name, DEFAULT_BRIGHTNESS_DRAFT] as const;
            }
          }),
        );
        const brightnessDraftMap = Object.fromEntries(brightnessEntries);
        const brightnessAppliedMap = Object.fromEntries(
          brightnessEntries.map(([name, value]) => [
            name,
            parseBrightness(value) ?? Number(DEFAULT_BRIGHTNESS_DRAFT),
          ]),
        );
        const initialLayoutDeviceName = resolveInitialLayoutDeviceName(configData, devicesData);
        let initialDeviceInfo: DeviceInfoResponse | null = null;
        let initialDeviceLayout: DeviceLayoutResponse | null = null;
        if (initialLayoutDeviceName) {
          try {
            [initialDeviceInfo, initialDeviceLayout] = await Promise.all([
              api.deviceInfo(initialLayoutDeviceName),
              api.deviceLayout(initialLayoutDeviceName),
            ]);
          } catch {
            // Keep dashboard usable even if initial layout preload fails.
          }
        }
        let audioBackendsData: AudioBackendsResponse = {
          current_audio_backend: configData.config?.visualizer_config.audio_backend ?? null,
          available_audio_backends: ["default"],
        };
        try {
          audioBackendsData = await api.audioBackends();
        } catch {
          // Keep UI usable even if backend enumeration is unavailable.
        }
        let visualizerStatusData: VisualizerStatusResponse | null = null;
        try {
          visualizerStatusData = await api.visualizerStatus();
        } catch {
          // Keep UI usable if stream status endpoint is temporarily unavailable.
        }

        if (!isMounted) {
          return;
        }

        setHealth(healthData);
        setConfig(configData);
        setDevices(devicesData);
        setBrightnessDraftByDevice(brightnessDraftMap);
        lastAppliedBrightnessRef.current = brightnessAppliedMap;
        setSelectedDeviceInfo(initialDeviceInfo);
        setSelectedDeviceLayout(initialDeviceLayout);
        const availableBackends =
          audioBackendsData.available_audio_backends.length > 0
            ? audioBackendsData.available_audio_backends
            : ["default"];
        setAudioBackends(availableBackends);
        setVisualizerStatus(visualizerStatusData);
        setPalettes(palettesData);
        setNowPlaying(nowPlayingData);
        hydrateVisualizerDrafts(
          configData,
          palettesData.palettes,
          availableBackends,
        );
        setLoadState("ready");
      } catch (error) {
        if (!isMounted) {
          return;
        }
        setLoadState("error");
        setErrorMessage(
          error instanceof Error ? error.message : "Unknown error contacting API",
        );
      }
    }

    void loadData();

    return () => {
      isMounted = false;
    };
  }, []);

  useEffect(() => {
    return () => {
      for (const timerId of Object.values(brightnessCommitTimersRef.current)) {
        window.clearTimeout(timerId);
      }
      brightnessCommitTimersRef.current = {};
    };
  }, []);

  useEffect(() => {
    if (!showLivePreview) {
      setLivePreviewColorsByPanel({});
      setLivePreviewDeviceName(null);
      return;
    }

    let cancelled = false;
    let timerId: number | undefined;
    const pollPreview = async () => {
      try {
        const preview = await api.visualizerPreview();
        if (cancelled) {
          return;
        }
        setLivePreviewDeviceName(preview.device?.name ?? null);
        setLivePreviewColorsByPanel(
          Object.fromEntries(preview.panel_colors.map((entry) => [entry.panel_id, entry.rgb])),
        );
      } catch {
        if (!cancelled) {
          setLivePreviewColorsByPanel({});
          setLivePreviewDeviceName(null);
        }
      } finally {
        if (!cancelled) {
          timerId = window.setTimeout(() => void pollPreview(), 180);
        }
      }
    };

    void pollPreview();

    return () => {
      cancelled = true;
      if (timerId !== undefined) {
        window.clearTimeout(timerId);
      }
    };
  }, [showLivePreview]);

  useEffect(() => {
    let cancelled = false;
    let timerId: number | undefined;
    const pollNowPlaying = async () => {
      try {
        const snapshot = await api.nowPlaying();
        if (!cancelled) {
          setNowPlaying(snapshot);
        }
      } catch {
        // Keep previous now-playing snapshot visible if polling fails.
      } finally {
        if (!cancelled) {
          timerId = window.setTimeout(() => void pollNowPlaying(), 1200);
        }
      }
    };

    void pollNowPlaying();

    return () => {
      cancelled = true;
      if (timerId !== undefined) {
        window.clearTimeout(timerId);
      }
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    let timerId: number | undefined;
    const pollVisualizerStatus = async () => {
      try {
        const snapshot = await api.visualizerStatus();
        if (!cancelled) {
          setVisualizerStatus(snapshot);
        }
      } catch {
        // Keep previous status visible if polling fails.
      } finally {
        if (!cancelled) {
          timerId = window.setTimeout(() => void pollVisualizerStatus(), 1200);
        }
      }
    };

    void pollVisualizerStatus();

    return () => {
      cancelled = true;
      if (timerId !== undefined) {
        window.clearTimeout(timerId);
      }
    };
  }, []);

  async function handleLoadDeviceDetails(name: string) {
    try {
      setErrorMessage(null);
      setLoadingDeviceName(name);
      const [info, layout] = await Promise.all([
        api.deviceInfo(name),
        api.deviceLayout(name),
      ]);
      setSelectedDeviceInfo(info);
      setSelectedDeviceLayout(layout);
    } catch (error) {
      setErrorMessage(
        error instanceof Error
          ? error.message
          : "Failed to load device info and layout",
      );
    } finally {
      setLoadingDeviceName(null);
    }
  }

  async function handleSetState(
    name: string,
    payload: DeviceStateUpdateRequest,
    actionLabel: string,
  ) {
    try {
      setErrorMessage(null);
      setActionMessage(null);
      setUpdatingDeviceName(name);
      await api.setDeviceState(name, payload);
      setActionMessage(`${actionLabel} applied on ${name}`);

      if (selectedDeviceInfo?.device.name === name) {
        const refreshedInfo = await api.deviceInfo(name);
        setSelectedDeviceInfo(refreshedInfo);
      }
    } catch (error) {
      setErrorMessage(
        error instanceof Error ? error.message : "Failed to update device state",
      );
    } finally {
      setUpdatingDeviceName(null);
    }
  }

  function hydrateVisualizerDrafts(
    nextConfig: ConfigResponse,
    nextPalettes: PaletteEntry[],
    nextAudioBackends: string[],
  ) {
    const visualizer = nextConfig.config?.visualizer_config;
    setEffectDraft(normalizeEffect(visualizer?.effect));
    const inferredPalette = inferPaletteName(visualizer?.colors, nextPalettes);
    setPaletteDraft(inferredPalette ?? "");
    setSortDraft({
      primary_axis: visualizer?.primary_axis === "X" ? "X" : "Y",
      sort_primary: visualizer?.sort_primary === "Desc" ? "Desc" : "Asc",
      sort_secondary: visualizer?.sort_secondary === "Desc" ? "Desc" : "Asc",
    });

    const configuredBackend = visualizer?.audio_backend ?? "default";
    const resolvedBackend = configuredBackend.trim().length > 0 ? configuredBackend : "default";
    const hasDefaultBackend = nextAudioBackends.includes("default");
    const backendIsAvailable = nextAudioBackends.includes(resolvedBackend);
    const draftBackend =
      backendIsAvailable || !hasDefaultBackend ? resolvedBackend : "default";
    setSettingsDraft({
      audio_backend: draftBackend,
      freq_min: String(visualizer?.freq_range?.[0] ?? 20),
      freq_max: String(visualizer?.freq_range?.[1] ?? 4500),
      default_gain: String(visualizer?.default_gain ?? 1),
      transition_time: formatTenths((visualizer?.transition_time ?? 2) / 10),
      time_window: formatTenths(visualizer?.time_window ?? 0.1875),
    });

    if (
      resolvedBackend !== "default" &&
      !backendIsAvailable &&
      hasDefaultBackend
    ) {
      setActionMessage((prev) =>
        prev ??
        `Configured audio backend "${resolvedBackend}" is not currently available. Using "default".`,
      );
    }
  }

  async function applyEffect(nextEffect: EffectOption) {
    try {
      setErrorMessage(null);
      setActionMessage(null);
      setSavingConfigSection("effect");
      const updated = await api.setVisualizerEffect(nextEffect);
      setConfig(updated);
      hydrateVisualizerDrafts(updated, palettes?.palettes ?? [], audioBackends);
      setActionMessage(`Effect set to ${nextEffect}`);
    } catch (error) {
      setErrorMessage(error instanceof Error ? error.message : "Failed to update effect");
    } finally {
      setSavingConfigSection(null);
    }
  }

  async function applyPalette(nextPalette: string) {
    try {
      setErrorMessage(null);
      setActionMessage(null);
      setSavingConfigSection("palette");
      const updated = await api.setVisualizerPalette(nextPalette);
      setConfig(updated);
      hydrateVisualizerDrafts(updated, palettes?.palettes ?? [], audioBackends);
      setActionMessage(`Palette set to ${nextPalette}`);
    } catch (error) {
      setErrorMessage(error instanceof Error ? error.message : "Failed to update palette");
    } finally {
      setSavingConfigSection(null);
    }
  }

  async function applySort(nextSort: VisualizerSortUpdateRequest) {
    try {
      setErrorMessage(null);
      setActionMessage(null);
      setSavingConfigSection("sort");
      const updated = await api.setVisualizerSort(nextSort);
      setConfig(updated);
      hydrateVisualizerDrafts(updated, palettes?.palettes ?? [], audioBackends);
      setActionMessage(
        `Sort updated (${nextSort.primary_axis}, ${nextSort.sort_primary}/${nextSort.sort_secondary})`,
      );
    } catch (error) {
      setErrorMessage(error instanceof Error ? error.message : "Failed to update sort");
    } finally {
      setSavingConfigSection(null);
    }
  }

  async function applySettingsPatch(
    payload: VisualizerSettingsUpdateRequest,
    successMessage: string,
  ) {
    try {
      setErrorMessage(null);
      setActionMessage(null);
      setSavingConfigSection("settings");
      const updated = await api.setVisualizerSettings(payload);
      setConfig(updated);
      hydrateVisualizerDrafts(updated, palettes?.palettes ?? [], audioBackends);
      setActionMessage(successMessage);
    } catch (error) {
      setErrorMessage(
        error instanceof Error ? error.message : "Failed to update visualizer settings",
      );
    } finally {
      setSavingConfigSection(null);
    }
  }

  function handleEffectChange(nextEffect: EffectOption) {
    setEffectDraft(nextEffect);
    if (nextEffect === effectDraft) {
      return;
    }
    void applyEffect(nextEffect);
  }

  function handlePaletteChange(nextPalette: string) {
    setPaletteDraft(nextPalette);
    if (!nextPalette) {
      setErrorMessage("Current colors are custom. Select a named palette to apply.");
      return;
    }
    if (nextPalette === paletteDraft) {
      return;
    }
    void applyPalette(nextPalette);
  }

  function handleSortAxisChange(primary_axis: "X" | "Y") {
    setSortDraft((prev) => {
      if (prev.primary_axis === primary_axis) {
        return prev;
      }
      const next = { ...prev, primary_axis };
      void applySort(next);
      return next;
    });
  }

  function handleSortPrimaryChange(sort_primary: "Asc" | "Desc") {
    setSortDraft((prev) => {
      if (prev.sort_primary === sort_primary) {
        return prev;
      }
      const next = { ...prev, sort_primary };
      void applySort(next);
      return next;
    });
  }

  function handleSortSecondaryChange(sort_secondary: "Asc" | "Desc") {
    setSortDraft((prev) => {
      if (prev.sort_secondary === sort_secondary) {
        return prev;
      }
      const next = { ...prev, sort_secondary };
      void applySort(next);
      return next;
    });
  }

  function handleAudioBackendChange(nextBackend: string) {
    const normalizedBackend = nextBackend.trim() || "default";
    setSettingsDraft((prev) => ({
      ...prev,
      audio_backend: normalizedBackend,
    }));
    if ((visualizerConfig?.audio_backend ?? "default") === normalizedBackend) {
      return;
    }
    void applySettingsPatch(
      { audio_backend: normalizedBackend },
      `Audio backend set to ${normalizedBackend}.`,
    );
  }

  function handleFreqRangeBlur() {
    const freqMin = parseInteger(settingsDraft.freq_min);
    const freqMax = parseInteger(settingsDraft.freq_max);
    if (freqMin === null || freqMax === null) {
      setErrorMessage("Frequency range must use integer values.");
      return;
    }
    if (freqMin < 0 || freqMax < 0 || freqMin > 65535 || freqMax > 65535 || freqMin >= freqMax) {
      setErrorMessage("Frequency range must be 0-65535 with min < max.");
      return;
    }
    const currentMin = visualizerConfig?.freq_range?.[0] ?? 20;
    const currentMax = visualizerConfig?.freq_range?.[1] ?? 4500;
    if (currentMin === freqMin && currentMax === freqMax) {
      return;
    }
    void applySettingsPatch(
      { freq_range: [freqMin, freqMax] },
      `Frequency range set to ${freqMin}-${freqMax} Hz.`,
    );
  }

  function handleDefaultGainBlur() {
    const defaultGain = parseNonNegativeFloat(settingsDraft.default_gain);
    if (defaultGain === null) {
      setErrorMessage("Default gain must be a finite number >= 0.");
      return;
    }
    const currentGain = visualizerConfig?.default_gain ?? 1;
    if (Math.abs(currentGain - defaultGain) < 1e-6) {
      return;
    }
    void applySettingsPatch({ default_gain: defaultGain }, `Default gain set to ${defaultGain}.`);
  }

  function handleTransitionBlur() {
    const transitionSeconds = parsePositiveFloat(settingsDraft.transition_time);
    if (transitionSeconds === null || transitionSeconds < 0.1 || transitionSeconds > 1.0) {
      setErrorMessage("Transition time must be between 0.1s and 1.0s.");
      return;
    }
    const transitionTenths = transitionSeconds * 10;
    if (Math.abs(transitionTenths - Math.round(transitionTenths)) > 1e-6) {
      setErrorMessage("Transition time must use 0.1 second steps.");
      return;
    }
    const transitionTime = Math.round(transitionTenths);
    const currentTransition = visualizerConfig?.transition_time ?? 2;
    if (currentTransition === transitionTime) {
      return;
    }
    void applySettingsPatch(
      { transition_time: transitionTime },
      `Transition time set to ${formatTenths(transitionSeconds)}s.`,
    );
  }

  function handleTimeWindowBlur() {
    const timeWindow = parsePositiveFloat(settingsDraft.time_window);
    if (timeWindow === null || timeWindow < 0.1 || timeWindow > 1.0) {
      setErrorMessage("Time window must be between 0.1s and 1.0s.");
      return;
    }
    const timeWindowTenths = timeWindow * 10;
    if (Math.abs(timeWindowTenths - Math.round(timeWindowTenths)) > 1e-6) {
      setErrorMessage("Time window must use 0.1 second steps.");
      return;
    }
    const normalizedTimeWindow = Math.round(timeWindowTenths) / 10;
    const currentTimeWindow = visualizerConfig?.time_window ?? 0.1875;
    if (Math.abs(currentTimeWindow - normalizedTimeWindow) < 1e-6) {
      return;
    }
    void applySettingsPatch(
      { time_window: normalizedTimeWindow },
      `Time window set to ${formatTenths(normalizedTimeWindow)}s.`,
    );
  }

  async function handleSaveRuntimeConfig() {
    try {
      setErrorMessage(null);
      setActionMessage(null);
      setSavingConfigSection("persist");
      const updated = await api.saveConfig();
      setConfig(updated);
      hydrateVisualizerDrafts(updated, palettes?.palettes ?? [], audioBackends);
      setActionMessage("Runtime config saved to config.toml.");
    } catch (error) {
      setErrorMessage(
        error instanceof Error ? error.message : "Failed to save runtime config",
      );
    } finally {
      setSavingConfigSection(null);
    }
  }

  async function handleNowPlayingDrivePaletteToggle(enabled: boolean) {
    try {
      setErrorMessage(null);
      setActionMessage(null);
      const updated = await api.setNowPlayingSettings({
        drive_visualizer_palette: enabled,
      });
      setNowPlaying(updated);
      setActionMessage(
        enabled
          ? "Now playing palette mode enabled."
          : "Now playing palette mode disabled.",
      );
    } catch (error) {
      setErrorMessage(
        error instanceof Error ? error.message : "Failed to update now playing settings",
      );
    }
  }

  function getBrightnessDraft(name: string): string {
    return brightnessDraftByDevice[name] ?? DEFAULT_BRIGHTNESS_DRAFT;
  }

  function handleBrightnessSliderChange(name: string, value: string) {
    setBrightnessDraftByDevice((prev) => ({ ...prev, [name]: value }));
    const parsed = parseBrightness(value);
    if (parsed !== null) {
      scheduleBrightnessUpdate(name, parsed);
    }
  }

  function handleBrightnessInputChange(name: string, value: string) {
    if (!isValidBrightnessInput(value)) {
      return;
    }
    setBrightnessDraftByDevice((prev) => ({ ...prev, [name]: value }));
  }

  function handleBrightnessInputBlur(name: string) {
    const parsed = parseBrightness(getBrightnessDraft(name));
    if (parsed === null) {
      setErrorMessage("Brightness must be an integer between 0 and 100.");
      return;
    }
    scheduleBrightnessUpdate(name, parsed);
  }

  function scheduleBrightnessUpdate(name: string, brightness: number) {
    const timers = brightnessCommitTimersRef.current;
    const existingTimer = timers[name];
    if (existingTimer !== undefined) {
      window.clearTimeout(existingTimer);
    }
    timers[name] = window.setTimeout(() => {
      delete timers[name];
      void commitBrightnessUpdate(name, brightness);
    }, 120);
  }

  async function commitBrightnessUpdate(name: string, brightness: number) {
    const previouslyApplied = lastAppliedBrightnessRef.current[name];
    if (previouslyApplied === brightness) {
      return;
    }

    try {
      setErrorMessage(null);
      await api.setDeviceState(name, { brightness });
      lastAppliedBrightnessRef.current[name] = brightness;
      setActionMessage(`Brightness ${brightness}% applied on ${name}`);
    } catch (error) {
      setErrorMessage(
        error instanceof Error ? error.message : "Failed to update device brightness",
      );
    }
  }

  return (
    <main className="mx-auto min-h-screen w-full max-w-6xl p-5 pb-16 sm:p-8">
      <header className="mb-8 flex flex-col gap-4 sm:flex-row sm:items-end sm:justify-between">
        <div>
          <p className="mb-2 text-xs uppercase tracking-[0.2em] text-muted-foreground">
            Audioleaf Web Control
          </p>
          <h1 className="font-display text-4xl font-semibold text-foreground sm:text-5xl">
            Axum + React Dashboard
          </h1>
        </div>
        <div className="flex items-center gap-2">
          <Badge variant={loadState === "ready" ? "default" : "secondary"}>
            {loadState === "ready" ? "API Connected" : "Connecting"}
          </Badge>
          {health ? <Badge variant="outline">v{health.version}</Badge> : null}
        </div>
      </header>

      {errorMessage ? (
        <Card className="mb-6 border-destructive/30 bg-destructive/5">
          <CardContent className="pt-5 text-sm text-destructive">
            {errorMessage}
          </CardContent>
        </Card>
      ) : null}

      {actionMessage ? (
        <Card className="mb-6 border-primary/25 bg-primary/5">
          <CardContent className="pt-5 text-sm text-foreground">{actionMessage}</CardContent>
        </Card>
      ) : null}

      <section className="grid gap-6 lg:grid-cols-3">
        <Card>
          <CardHeader>
            <CardTitle>Runtime</CardTitle>
            <CardDescription>
              Backend health, file paths, and explicit runtime config persistence
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-3 text-sm">
            <DataRow label="Health">
              {health?.status ?? (loadState === "loading" ? "Loading..." : "-")}
            </DataRow>
            <DataRow label="Stream status">
              <Badge variant={visualizerStatus?.status === "Healthy" ? "default" : "secondary"}>
                {visualizerStatus?.status ?? (loadState === "loading" ? "Loading..." : "Unknown")}
              </Badge>
            </DataRow>
            <DataRow label="Config file">
              {config?.paths.config_file_path ?? "-"}
            </DataRow>
            <DataRow label="Devices file">
              {config?.paths.devices_file_path ?? "-"}
            </DataRow>
            <DataRow label="Default device">
              {config?.config?.default_nl_device_name ?? "Not configured"}
            </DataRow>
            <div className="rounded-md border border-border/70 bg-background/60 p-3">
              <p className="mb-2 text-xs uppercase tracking-[0.12em] text-muted-foreground">
                Runtime Config
              </p>
              <p className="mb-3 text-sm text-muted-foreground">
                Live changes are in-memory until you save them.
              </p>
              <Button
                size="sm"
                variant="secondary"
                disabled={savingConfigSection !== null}
                onClick={() => void handleSaveRuntimeConfig()}
              >
                Save Runtime Config
              </Button>
            </div>
          </CardContent>
        </Card>

        <Card className="lg:col-span-2">
          <CardHeader>
            <CardTitle>Visualizer Settings</CardTitle>
            <CardDescription>
              Loaded from live runtime state. Update effect, palette, sort, and core visualizer
              settings without immediate disk writes.
            </CardDescription>
          </CardHeader>
          <CardContent>
            {visualizerConfig ? (
              <div className="space-y-4">
                <div className="grid gap-3 sm:grid-cols-2">
                  <SettingCell
                    label="Audio backend"
                    value={visualizerConfig.audio_backend ?? "default"}
                  />
                  <SettingCell
                    label="Frequency range"
                    value={
                      visualizerConfig.freq_range
                        ? `${visualizerConfig.freq_range[0]}-${visualizerConfig.freq_range[1]} Hz`
                        : "-"
                    }
                  />
                  <SettingCell
                    label="Default gain"
                    value={
                      visualizerConfig.default_gain !== null
                        ? String(visualizerConfig.default_gain)
                        : "-"
                    }
                  />
                  <SettingCell
                    label="Transition time"
                    value={
                      visualizerConfig.transition_time !== null
                        ? `${formatTenths(visualizerConfig.transition_time / 10)}s`
                        : "-"
                    }
                  />
                  <SettingCell
                    label="Time window"
                    value={
                      visualizerConfig.time_window !== null
                        ? `${formatTenths(visualizerConfig.time_window)}s`
                        : "-"
                    }
                  />
                  <SettingCell
                    label="Effect"
                    value={visualizerConfig.effect ?? "Spectrum"}
                  />
                  <SettingCell
                    label="Primary axis"
                    value={visualizerConfig.primary_axis ?? "Y"}
                  />
                  <SettingCell
                    label="Sort (primary / secondary)"
                    value={`${visualizerConfig.sort_primary ?? "Asc"} / ${visualizerConfig.sort_secondary ?? "Asc"}`}
                  />
                </div>
                <div>
                  <p className="mb-2 text-xs uppercase tracking-[0.15em] text-muted-foreground">
                    Configured Colors
                  </p>
                  {visualizerConfig.colors?.length ? (
                    <div className="flex flex-wrap items-center gap-1.5">
                      {visualizerConfig.colors.map(([r, g, b], idx) => (
                        <span
                          key={`config-color-${idx}`}
                          className="h-6 w-8 rounded-sm border border-border/70"
                          style={{ backgroundColor: `rgb(${r}, ${g}, ${b})` }}
                          title={`rgb(${r}, ${g}, ${b})`}
                        />
                      ))}
                    </div>
                  ) : (
                    <p className="text-sm text-muted-foreground">No colors configured.</p>
                  )}
                </div>

                <div className="rounded-md border border-border/70 bg-background/60 p-3">
                  <p className="mb-2 text-xs uppercase tracking-[0.12em] text-muted-foreground">
                    Effect
                  </p>
                  <div className="flex flex-wrap items-center gap-2">
                    <select
                      className="h-10 min-w-[180px] rounded-md border border-input bg-background px-3 text-sm"
                      value={effectDraft}
                      onChange={(event) =>
                        handleEffectChange(event.currentTarget.value as EffectOption)
                      }
                      disabled={savingConfigSection !== null}
                    >
                      {EFFECT_OPTIONS.map((effect) => (
                        <option key={effect} value={effect}>
                          {effect}
                        </option>
                      ))}
                    </select>
                    <p className="text-xs text-muted-foreground">Applies immediately on change.</p>
                  </div>
                </div>

                <div className="rounded-md border border-border/70 bg-background/60 p-3">
                  <p className="mb-2 text-xs uppercase tracking-[0.12em] text-muted-foreground">
                    Palette
                  </p>
                  <div className="flex flex-wrap items-center gap-2">
                    <select
                      className="h-10 min-w-[220px] rounded-md border border-input bg-background px-3 text-sm"
                      value={paletteDraft}
                      onChange={(event) => handlePaletteChange(event.currentTarget.value)}
                      disabled={savingConfigSection !== null}
                    >
                      <option value="">Custom (no preset match)</option>
                      {(palettes?.palettes ?? []).map((palette) => (
                        <option key={palette.name} value={palette.name}>
                          {palette.name}
                        </option>
                      ))}
                    </select>
                    <p className="text-xs text-muted-foreground">Applies immediately on change.</p>
                  </div>
                </div>

                <div className="rounded-md border border-border/70 bg-background/60 p-3">
                  <p className="mb-2 text-xs uppercase tracking-[0.12em] text-muted-foreground">
                    Sort
                  </p>
                  <div className="grid gap-2 sm:grid-cols-3 sm:items-center">
                    <select
                      className="h-10 rounded-md border border-input bg-background px-3 text-sm"
                      value={sortDraft.primary_axis}
                      onChange={(event) =>
                        handleSortAxisChange(event.currentTarget.value as "X" | "Y")
                      }
                      disabled={savingConfigSection !== null}
                    >
                      <option value="X">Axis: X</option>
                      <option value="Y">Axis: Y</option>
                    </select>
                    <select
                      className="h-10 rounded-md border border-input bg-background px-3 text-sm"
                      value={sortDraft.sort_primary}
                      onChange={(event) =>
                        handleSortPrimaryChange(event.currentTarget.value as "Asc" | "Desc")
                      }
                      disabled={savingConfigSection !== null}
                    >
                      <option value="Asc">Primary: Asc</option>
                      <option value="Desc">Primary: Desc</option>
                    </select>
                    <select
                      className="h-10 rounded-md border border-input bg-background px-3 text-sm"
                      value={sortDraft.sort_secondary}
                      onChange={(event) =>
                        handleSortSecondaryChange(event.currentTarget.value as "Asc" | "Desc")
                      }
                      disabled={savingConfigSection !== null}
                    >
                      <option value="Asc">Secondary: Asc</option>
                      <option value="Desc">Secondary: Desc</option>
                    </select>
                  </div>
                  <p className="mt-2 text-xs text-muted-foreground">
                    Applies immediately when a value changes.
                  </p>
                </div>

                <div className="rounded-md border border-border/70 bg-background/60 p-3">
                  <p className="mb-2 text-xs uppercase tracking-[0.12em] text-muted-foreground">
                    Core Visualizer Settings
                  </p>
                  <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
                    <label className="space-y-1 text-xs text-muted-foreground">
                      <span>Audio backend</span>
                      <select
                        className="h-10 w-full rounded-md border border-input bg-background px-3 text-sm text-foreground"
                        value={settingsDraft.audio_backend}
                        onChange={(event) => {
                          handleAudioBackendChange(event.currentTarget.value);
                        }}
                        disabled={savingConfigSection !== null}
                      >
                        {availableBackendOptions.map((backend) => (
                          <option key={backend} value={backend}>
                            {backend}
                          </option>
                        ))}
                      </select>
                    </label>

                    <label className="space-y-1 text-xs text-muted-foreground">
                      <span>Frequency min (Hz)</span>
                      <input
                        type="text"
                        inputMode="numeric"
                        value={settingsDraft.freq_min}
                        onChange={(event) => {
                          const value = event.currentTarget.value;
                          setSettingsDraft((prev) => ({
                            ...prev,
                            freq_min: value,
                          }));
                        }}
                        onBlur={handleFreqRangeBlur}
                        disabled={savingConfigSection !== null}
                        className="h-10 w-full rounded-md border border-input bg-background px-3 text-sm text-foreground"
                      />
                    </label>

                    <label className="space-y-1 text-xs text-muted-foreground">
                      <span>Frequency max (Hz)</span>
                      <input
                        type="text"
                        inputMode="numeric"
                        value={settingsDraft.freq_max}
                        onChange={(event) => {
                          const value = event.currentTarget.value;
                          setSettingsDraft((prev) => ({
                            ...prev,
                            freq_max: value,
                          }));
                        }}
                        onBlur={handleFreqRangeBlur}
                        disabled={savingConfigSection !== null}
                        className="h-10 w-full rounded-md border border-input bg-background px-3 text-sm text-foreground"
                      />
                    </label>

                    <label className="space-y-1 text-xs text-muted-foreground">
                      <span>Default gain</span>
                      <input
                        type="text"
                        inputMode="decimal"
                        value={settingsDraft.default_gain}
                        onChange={(event) => {
                          const value = event.currentTarget.value;
                          setSettingsDraft((prev) => ({
                            ...prev,
                            default_gain: value,
                          }));
                        }}
                        onBlur={handleDefaultGainBlur}
                        disabled={savingConfigSection !== null}
                        className="h-10 w-full rounded-md border border-input bg-background px-3 text-sm text-foreground"
                      />
                    </label>

                    <label className="space-y-1 text-xs text-muted-foreground">
                      <span>Transition time (seconds)</span>
                      <input
                        type="number"
                        min={0.1}
                        max={1}
                        step={0.1}
                        value={settingsDraft.transition_time}
                        onChange={(event) => {
                          const value = event.currentTarget.value;
                          setSettingsDraft((prev) => ({
                            ...prev,
                            transition_time: value,
                          }));
                        }}
                        onBlur={handleTransitionBlur}
                        disabled={savingConfigSection !== null}
                        className="h-10 w-full rounded-md border border-input bg-background px-3 text-sm text-foreground"
                      />
                    </label>

                    <label className="space-y-1 text-xs text-muted-foreground">
                      <span>Time window (seconds)</span>
                      <input
                        type="number"
                        min={0.1}
                        max={1}
                        step={0.1}
                        value={settingsDraft.time_window}
                        onChange={(event) => {
                          const value = event.currentTarget.value;
                          setSettingsDraft((prev) => ({
                            ...prev,
                            time_window: value,
                          }));
                        }}
                        onBlur={handleTimeWindowBlur}
                        disabled={savingConfigSection !== null}
                        className="h-10 w-full rounded-md border border-input bg-background px-3 text-sm text-foreground"
                      />
                    </label>
                  </div>
                  <p className="mt-2 text-xs text-muted-foreground">
                    Applies on focus loss for numeric fields and immediately for backend selection.
                  </p>
                </div>
              </div>
            ) : (
              <p className="text-sm text-muted-foreground">
                Visualizer config not found in your config file.
              </p>
            )}
          </CardContent>
        </Card>

        <Card className="lg:col-span-3">
          <CardHeader>
            <CardTitle>Now Playing (AirPlay Metadata)</CardTitle>
            <CardDescription>
              Track and artwork data from Shairport metadata pipe{" "}
              <code>
                {nowPlaying?.metadata_pipe_path ?? "/tmp/shairport-sync-metadata"}
              </code>
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            <div className="flex flex-wrap items-center gap-3">
              <Badge variant={nowPlaying?.reader_running ? "default" : "secondary"}>
                {nowPlaying?.reader_running ? "Reader Running" : "Reader Waiting"}
              </Badge>
              <label className="inline-flex items-center gap-3 text-sm text-muted-foreground">
                <Switch
                  checked={nowPlaying?.drive_visualizer_palette ?? false}
                  onChange={(event) =>
                    void handleNowPlayingDrivePaletteToggle(event.currentTarget.checked)
                  }
                  disabled={!nowPlaying}
                />
                Drive visualizer palette from artwork colors
              </label>
            </div>

            {nowPlaying?.last_error ? (
              <p className="rounded-md border border-destructive/30 bg-destructive/5 p-3 text-xs text-destructive">
                {nowPlaying.last_error}
              </p>
            ) : null}

            <div className="grid gap-4 lg:grid-cols-[180px_1fr]">
              <div className="overflow-hidden rounded-md border border-border/70 bg-background/70">
                {nowPlaying?.artwork_available ? (
                  <img
                    src={apiAssetUrl(
                      `/api/now-playing/artwork?g=${nowPlaying.artwork_generation}`,
                    )}
                    alt="Album artwork"
                    className="h-[180px] w-full object-cover"
                  />
                ) : (
                  <div className="flex h-[180px] items-center justify-center px-3 text-center text-xs text-muted-foreground">
                    No artwork available yet
                  </div>
                )}
              </div>
              <div className="grid gap-2 text-sm sm:grid-cols-2">
                <SettingCell
                  label="Title"
                  value={nowPlaying?.track?.title ?? "No active track"}
                />
                <SettingCell label="Artist" value={nowPlaying?.track?.artist ?? "-"} />
                <SettingCell label="Album" value={nowPlaying?.track?.album ?? "-"} />
                <SettingCell
                  label="Source"
                  value={nowPlaying?.track?.source_name ?? nowPlaying?.track?.source_ip ?? "-"}
                />
              </div>
            </div>

            <div>
              <p className="mb-2 text-xs uppercase tracking-[0.12em] text-muted-foreground">
                Extracted Artwork Colors
              </p>
              {nowPlaying?.palette_colors.length ? (
                <div className="flex flex-wrap items-center gap-1.5">
                  {nowPlaying.palette_colors.map(([r, g, b], idx) => (
                    <span
                      key={`now-playing-color-${idx}`}
                      className="h-8 w-10 rounded-sm border border-border/70"
                      style={{ backgroundColor: `rgb(${r}, ${g}, ${b})` }}
                      title={`rgb(${r}, ${g}, ${b})`}
                    />
                  ))}
                </div>
              ) : (
                <p className="text-sm text-muted-foreground">
                  Artwork palette unavailable. Start playback through Shairport Sync.
                </p>
              )}
            </div>
          </CardContent>
        </Card>

        <Card className="lg:col-span-3">
          <CardHeader>
            <CardTitle>Devices</CardTitle>
            <CardDescription>
              Known Nanoleaf devices loaded from your devices TOML
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            {devices?.devices.length ? (
              devices.devices.map((device) => {
                const isUpdating = updatingDeviceName === device.name;
                const isLoadingDetails = loadingDeviceName === device.name;
                const isBusy = isUpdating || isLoadingDetails;
                const brightnessDraft = getBrightnessDraft(device.name);
                return (
                  <div
                    key={device.name}
                    className="rounded-md border border-border/70 bg-background/70 p-3"
                  >
                    <div className="mb-3 flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
                      <div>
                        <p className="font-medium">{device.name}</p>
                        <p className="text-sm text-muted-foreground">{device.ip}</p>
                      </div>
                      <Button
                        size="sm"
                        onClick={() => void handleLoadDeviceDetails(device.name)}
                        disabled={isBusy}
                      >
                        {isLoadingDetails ? "Loading..." : "Load Details"}
                      </Button>
                    </div>

                    <div className="flex flex-wrap items-center gap-2">
                      <Button
                        size="sm"
                        variant="secondary"
                        disabled={isBusy}
                        onClick={() =>
                          void handleSetState(device.name, { power_on: true }, "Power on")
                        }
                      >
                        Power On
                      </Button>
                      <Button
                        size="sm"
                        variant="ghost"
                        disabled={isBusy}
                        onClick={() =>
                          void handleSetState(device.name, { power_on: false }, "Power off")
                        }
                      >
                        Power Off
                      </Button>
                    </div>

                    <div className="mt-3 rounded-md border border-border/60 bg-card p-3">
                      <p className="mb-2 text-xs uppercase tracking-[0.15em] text-muted-foreground">
                        Brightness
                      </p>
                      <div className="grid gap-2 sm:grid-cols-[1fr_auto] sm:items-center">
                        <input
                          type="range"
                          min={0}
                          max={100}
                          step={1}
                          value={brightnessDraft === "" ? "0" : brightnessDraft}
                          onChange={(event) =>
                            handleBrightnessSliderChange(device.name, event.currentTarget.value)
                          }
                          disabled={isBusy}
                          className="w-full accent-[hsl(var(--primary))]"
                        />
                        <input
                          type="text"
                          inputMode="numeric"
                          value={brightnessDraft}
                          onChange={(event) =>
                            handleBrightnessInputChange(device.name, event.currentTarget.value)
                          }
                          onBlur={() => handleBrightnessInputBlur(device.name)}
                          disabled={isBusy}
                          className="h-10 w-20 rounded-md border border-input bg-background px-3 text-sm"
                          aria-label={`Brightness value for ${device.name}`}
                        />
                      </div>
                      <p className="mt-2 text-xs text-muted-foreground">
                        Slider applies while dragging. Typed values apply on focus loss.
                      </p>
                    </div>
                  </div>
                );
              })
            ) : (
              <p className="text-sm text-muted-foreground">
                {loadState === "loading"
                  ? "Loading devices..."
                  : "No known devices found yet. Pair one in the CLI first."}
              </p>
            )}
          </CardContent>
        </Card>

        <Card className="lg:col-span-3">
          <CardHeader>
            <CardTitle>All Palettes</CardTitle>
            <CardDescription>
              {palettes?.palettes.length ?? 0} palettes from <code>/api/palettes</code>
            </CardDescription>
          </CardHeader>
          <CardContent className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
            {palettes?.palettes.length ? (
              palettes.palettes.map((palette) => (
                <PaletteCard key={palette.name} palette={palette} />
              ))
            ) : (
              <p className="text-sm text-muted-foreground">
                {loadState === "loading" ? "Loading palettes..." : "No palettes found"}
              </p>
            )}
          </CardContent>
        </Card>
      </section>

      <section className="mt-6">
        <Card className="border-primary/40 shadow-card">
          <CardHeader>
            <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
              <div>
                <CardTitle className="text-xl">Panel Layout</CardTitle>
                <CardDescription>
                  {selectedDeviceLayout
                    ? `${selectedDeviceLayout.device.name} (${selectedDeviceLayout.device.ip}) • Global orientation ${selectedDeviceLayout.global_orientation}° • ${selectedDeviceLayout.panels.length} panels`
                    : "Default device layout is loading or unavailable."}
                </CardDescription>
              </div>
              <label className="inline-flex items-center gap-3 text-sm text-muted-foreground">
                <Switch
                  checked={showLivePreview}
                  onChange={(event) => setShowLivePreview(event.currentTarget.checked)}
                />
                Show Live Animation
              </label>
            </div>
          </CardHeader>
          <CardContent>
            {selectedDeviceLayout ? (
              <DeviceLayoutViewer
                layout={selectedDeviceLayout}
                livePreviewEnabled={
                  showLivePreview && livePreviewDeviceName === selectedDeviceLayout.device.name
                }
                livePreviewColorsByPanel={livePreviewColorsByPanel}
              />
            ) : (
              <p className="text-sm text-muted-foreground">
                No layout loaded yet. Use Load Details on a device to load manually.
              </p>
            )}
          </CardContent>
        </Card>
      </section>

      {selectedDeviceInfo ? (
        <section className="mt-6">
          <Card>
            <CardHeader>
              <CardTitle>Selected Device Info</CardTitle>
              <CardDescription>
                {selectedDeviceInfo.device.name} ({selectedDeviceInfo.device.ip})
              </CardDescription>
            </CardHeader>
            <CardContent>
              <pre className="max-h-[420px] overflow-auto rounded-md bg-secondary/60 p-4 text-xs leading-5">
                {JSON.stringify(selectedDeviceInfo.info, null, 2)}
              </pre>
            </CardContent>
          </Card>
        </section>
      ) : null}
    </main>
  );
}

function DataRow({
  label,
  children,
}: {
  label: string;
  children: ReactNode;
}) {
  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between gap-4">
        <span className="text-muted-foreground">{label}</span>
        <span className="max-w-[65%] truncate text-right font-medium">{children}</span>
      </div>
      <Separator />
    </div>
  );
}

function SettingCell({ label, value }: { label: string; value: ReactNode }) {
  return (
    <div className="rounded-md border border-border/70 bg-background/60 p-3">
      <p className="mb-1 text-xs uppercase tracking-[0.12em] text-muted-foreground">{label}</p>
      <p className="text-sm font-medium">{value}</p>
    </div>
  );
}

function PaletteCard({ palette }: { palette: PaletteEntry }) {
  return (
    <div className="rounded-md border border-border/70 bg-background/70 p-3">
      <p className="mb-3 truncate text-sm font-medium">{palette.name}</p>
      <div className="flex items-center gap-1.5">
        {palette.colors.map(([r, g, b], idx) => (
          <span
            key={`${palette.name}-${idx}`}
            className="h-6 flex-1 rounded-sm"
            style={{ backgroundColor: `rgb(${r}, ${g}, ${b})` }}
            title={`rgb(${r}, ${g}, ${b})`}
          />
        ))}
      </div>
    </div>
  );
}

function DeviceLayoutViewer({
  layout,
  livePreviewEnabled,
  livePreviewColorsByPanel,
}: {
  layout: DeviceLayoutResponse;
  livePreviewEnabled: boolean;
  livePreviewColorsByPanel: Record<number, [number, number, number]>;
}) {
  const width = 1400;
  const height = 780;
  const padding = 56;

  if (!layout.panels.length) {
    return <p className="text-sm text-muted-foreground">No panel layout data found.</p>;
  }

  const minX = Math.min(...layout.panels.map((panel) => panel.x));
  const maxX = Math.max(...layout.panels.map((panel) => panel.x));
  const minY = Math.min(...layout.panels.map((panel) => panel.y));
  const maxY = Math.max(...layout.panels.map((panel) => panel.y));

  const centerX = (minX + maxX) / 2;
  const centerY = (minY + maxY) / 2;
  const angle = (-layout.global_orientation * Math.PI) / 180;

  const rotated = layout.panels.map((panel) => {
    const relX = panel.x - centerX;
    const relY = panel.y - centerY;
    const rx = relX * Math.cos(angle) - relY * Math.sin(angle);
    const ry = relX * Math.sin(angle) + relY * Math.cos(angle);
    return {
      panel,
      rx,
      ry,
      radius: panelBaseRadius(panel),
    };
  });

  const minRx = Math.min(...rotated.map((item) => item.rx - item.radius));
  const maxRx = Math.max(...rotated.map((item) => item.rx + item.radius));
  const minRy = Math.min(...rotated.map((item) => item.ry - item.radius));
  const maxRy = Math.max(...rotated.map((item) => item.ry + item.radius));

  const spanX = Math.max(maxRx - minRx, 1);
  const spanY = Math.max(maxRy - minRy, 1);
  const scale = Math.min((width - 2 * padding) / spanX, (height - 2 * padding) / spanY);

  const offsetX = (width - spanX * scale) / 2 - minRx * scale;
  const offsetY = (height - spanY * scale) / 2 - minRy * scale;

  const renderPanels = rotated.map(({ panel, rx, ry, radius }) => {
    const sx = rx * scale + offsetX;
    const sy = height - (ry * scale + offsetY);
    return {
      ...panel,
      sx,
      sy,
      scaledRadius: radius * scale,
    };
  });
  const lightPanels = renderPanels.filter((panel) => panel.side_length >= 1);
  const controllerPanels = renderPanels.filter((panel) => panel.side_length < 1);

  return (
    <div className="space-y-3">
      <svg
        viewBox={`0 0 ${width} ${height}`}
        className="h-auto w-full rounded-md border border-border/70 bg-background"
        role="img"
        aria-label={`Panel layout for ${layout.device.name}`}
        shapeRendering="geometricPrecision"
      >
        <defs>
          <filter id="panelShadow" x="-20%" y="-20%" width="140%" height="140%">
            <feDropShadow
              dx="0"
              dy="1.5"
              stdDeviation="1.5"
              floodColor="#000"
              floodOpacity="0.35"
            />
          </filter>
        </defs>
        {lightPanels.map((panel) => (
          <g
            key={`${panel.panel_id}-${panel.x}-${panel.y}`}
            filter="url(#panelShadow)"
          >
            <polygon
              points={buildPanelPolygonPoints(panel)}
              fill={
                livePreviewEnabled
                  ? panelFillColor(panel, livePreviewColorsByPanel[panel.panel_id])
                  : panelFillColor(panel)
              }
              stroke="hsl(var(--foreground) / 0.85)"
              strokeWidth={1.25}
              strokeLinejoin="round"
            />
            <title>
              Panel {panel.panel_id} • {panel.shape_type_name}
            </title>
          </g>
        ))}
        {controllerPanels.map((panel) => (
          <g
            key={`${panel.panel_id}-${panel.x}-${panel.y}-controller`}
            filter="url(#panelShadow)"
          >
            <polygon
              points={buildControllerTrapezoidPoints(panel, lightPanels)}
              fill={panelFillColor(panel)}
              stroke="hsl(var(--foreground) / 0.9)"
              strokeWidth={1.25}
              strokeLinejoin="round"
            />
            <title>Controller</title>
          </g>
        ))}
      </svg>
      <p className="text-xs text-muted-foreground">
        Rendering {layout.panels.length} panels from Nanoleaf layout data. Controller
        panels are shown as trapezoids. Live animation preview is{" "}
        {livePreviewEnabled ? "enabled" : "disabled"}.
      </p>
    </div>
  );
}

function panelBaseRadius(panel: DeviceLayoutPanel): number {
  if (panel.side_length < 1) {
    return 14;
  }
  if (panel.num_sides === 3) {
    return panel.side_length / Math.sqrt(3);
  }
  if (panel.num_sides === 4) {
    return panel.side_length / Math.sqrt(2);
  }
  if (panel.num_sides === 6) {
    return panel.side_length;
  }
  return Math.max(panel.side_length * 0.65, 20);
}

const PANEL_OFF_FILL = "hsl(0 0% 96% / 0.92)";
const PANEL_LIVE_DARK_THRESHOLD = 18;

function panelFillColor(
  panel: DeviceLayoutPanel,
  liveRgb?: [number, number, number],
): string {
  if (panel.side_length < 1) {
    return "hsl(var(--accent) / 0.95)";
  }
  if (liveRgb) {
    const [r, g, b] = liveRgb;
    if (Math.max(r, g, b) < PANEL_LIVE_DARK_THRESHOLD) {
      return PANEL_OFF_FILL;
    }
    return `rgb(${r}, ${g}, ${b})`;
  }
  return PANEL_OFF_FILL;
}

function buildPanelPolygonPoints(panel: {
  sx: number;
  sy: number;
  scaledRadius: number;
  orientation: number;
  num_sides: number;
}) {
  const sides = Math.max(3, Math.round(panel.num_sides || 4));
  const orientationRadians = (panel.orientation * Math.PI) / 180;
  const points: string[] = [];

  for (let index = 0; index < sides; index += 1) {
    const theta = orientationRadians + (2 * Math.PI * index) / sides;
    points.push(
      `${panel.sx + panel.scaledRadius * Math.cos(theta)},${panel.sy + panel.scaledRadius * Math.sin(theta)}`,
    );
  }

  return points.join(" ");
}

function buildControllerTrapezoidPoints(
  controller: {
    sx: number;
    sy: number;
    scaledRadius: number;
  },
  lightPanels: Array<{
    sx: number;
    sy: number;
    scaledRadius: number;
    orientation: number;
    num_sides: number;
  }>,
) {
  if (!lightPanels.length) {
    return buildPanelPolygonPoints({
      ...controller,
      orientation: 0,
      num_sides: 4,
    });
  }

  const nearestPanel = lightPanels.reduce((best, panel) => {
    const bestDistance = Math.hypot(best.sx - controller.sx, best.sy - controller.sy);
    const panelDistance = Math.hypot(panel.sx - controller.sx, panel.sy - controller.sy);
    return panelDistance < bestDistance ? panel : best;
  }, lightPanels[0]);

  const numSides = Math.max(3, Math.round(nearestPanel.num_sides || 4));
  const parentRadius = nearestPanel.scaledRadius;
  const parentOrientation = (nearestPanel.orientation * Math.PI) / 180;
  const angleToController = Math.atan2(
    controller.sy - nearestPanel.sy,
    controller.sx - nearestPanel.sx,
  );
  const anglePerSide = (2 * Math.PI) / numSides;

  let closestEdge = 0;
  let minAngleDiff = Number.POSITIVE_INFINITY;
  for (let index = 0; index < numSides; index += 1) {
    const vertexAngle = parentOrientation + index * anglePerSide;
    const rawDiff = Math.abs(angleToController - vertexAngle) % (2 * Math.PI);
    const angleDiff = Math.min(rawDiff, 2 * Math.PI - rawDiff);
    if (angleDiff < minAngleDiff) {
      minAngleDiff = angleDiff;
      closestEdge = index;
    }
  }

  const v1Angle = parentOrientation + closestEdge * anglePerSide;
  const v2Angle = parentOrientation + (closestEdge + 1) * anglePerSide;

  const v1x = nearestPanel.sx + parentRadius * Math.cos(v1Angle);
  const v1y = nearestPanel.sy + parentRadius * Math.sin(v1Angle);
  const v2x = nearestPanel.sx + parentRadius * Math.cos(v2Angle);
  const v2y = nearestPanel.sy + parentRadius * Math.sin(v2Angle);

  const edgeMidX = (v1x + v2x) / 2;
  const edgeMidY = (v1y + v2y) / 2;
  const perpDx = edgeMidX - nearestPanel.sx;
  const perpDy = edgeMidY - nearestPanel.sy;
  const perpLen = Math.hypot(perpDx, perpDy);
  const perpNormX = perpLen < 1 ? 0 : perpDx / perpLen;
  const perpNormY = perpLen < 1 ? -1 : perpDy / perpLen;

  const trapezoidHeight = Math.max(16, Math.min(28, parentRadius * 0.32));
  const narrowRatio = 0.6;

  const p1 = `${v1x},${v1y}`;
  const p2 = `${v2x},${v2y}`;
  const p3 = `${v2x + perpNormX * trapezoidHeight - (v2x - edgeMidX) * (1 - narrowRatio)},${v2y + perpNormY * trapezoidHeight - (v2y - edgeMidY) * (1 - narrowRatio)}`;
  const p4 = `${v1x + perpNormX * trapezoidHeight - (v1x - edgeMidX) * (1 - narrowRatio)},${v1y + perpNormY * trapezoidHeight - (v1y - edgeMidY) * (1 - narrowRatio)}`;

  return `${p1} ${p2} ${p3} ${p4}`;
}

export default App;
