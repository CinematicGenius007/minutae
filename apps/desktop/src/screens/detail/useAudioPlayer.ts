// Plays a meeting's two tracks (mic + system) in sync. The system track (or mic if it's the
// only one) is the clock; the other follows and is re-aligned when it drifts > 150 ms.
import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "../../lib/api";

export interface AudioPlayer {
  available: boolean;
  playing: boolean;
  currentMs: number;
  durationMs: number;
  speed: number;
  toggle: () => void;
  seek: (ms: number, play?: boolean) => void;
  setSpeed: (v: number) => void;
}

export function useAudioPlayer(paths: { mic: string | null; system: string | null } | null, fallbackMs: number): AudioPlayer {
  const [els, setEls] = useState<HTMLAudioElement[]>([]);
  const [playing, setPlaying] = useState(false);
  const [currentMs, setCurrentMs] = useState(0);
  const [durationMs, setDurationMs] = useState(fallbackMs);
  const [speed, setSpeedState] = useState(1);
  const pendingSeek = useRef<number | null>(null);

  const mic = paths?.mic ?? null;
  const system = paths?.system ?? null;

  useEffect(() => {
    let alive = true;
    const made: HTMLAudioElement[] = [];
    (async () => {
      for (const p of [system, mic]) {
        if (!p) continue;
        const url = await api.audioUrl(p);
        if (!url || !alive) continue;
        const a = new Audio();
        a.preload = "metadata";
        a.src = url;
        made.push(a);
      }
      if (alive) setEls(made);
    })();
    return () => {
      alive = false;
      for (const a of made) {
        a.pause();
        a.src = "";
      }
      setEls([]);
      setPlaying(false);
    };
  }, [mic, system]);

  useEffect(() => {
    const [master, ...rest] = els;
    if (!master) return;
    const onTime = () => {
      setCurrentMs(master.currentTime * 1000);
      for (const a of rest) if (Math.abs(a.currentTime - master.currentTime) > 0.15) a.currentTime = master.currentTime;
    };
    const onMeta = () => {
      if (Number.isFinite(master.duration)) setDurationMs(master.duration * 1000);
      if (pendingSeek.current != null) {
        for (const a of els) a.currentTime = pendingSeek.current / 1000;
        pendingSeek.current = null;
      }
    };
    const onPlay = () => setPlaying(true);
    const onPause = () => setPlaying(false);
    master.addEventListener("timeupdate", onTime);
    master.addEventListener("loadedmetadata", onMeta);
    master.addEventListener("play", onPlay);
    master.addEventListener("pause", onPause);
    master.addEventListener("ended", onPause);
    return () => {
      master.removeEventListener("timeupdate", onTime);
      master.removeEventListener("loadedmetadata", onMeta);
      master.removeEventListener("play", onPlay);
      master.removeEventListener("pause", onPause);
      master.removeEventListener("ended", onPause);
    };
  }, [els]);

  const play = useCallback(() => {
    for (const a of els) void a.play().catch(() => setPlaying(false));
  }, [els]);

  const toggle = useCallback(() => {
    if (!els.length) return;
    if (els[0]?.paused) play();
    else for (const a of els) a.pause();
  }, [els, play]);

  const seek = useCallback(
    (ms: number, andPlay = false) => {
      setCurrentMs(ms);
      if (!els.length) return;
      if (els[0] && els[0].readyState < 1) pendingSeek.current = ms;
      else for (const a of els) a.currentTime = ms / 1000;
      if (andPlay) play();
    },
    [els, play],
  );

  const setSpeed = useCallback(
    (v: number) => {
      setSpeedState(v);
      for (const a of els) a.playbackRate = v;
    },
    [els],
  );

  return { available: els.length > 0, playing, currentMs, durationMs: durationMs || fallbackMs, speed, toggle, seek, setSpeed };
}
