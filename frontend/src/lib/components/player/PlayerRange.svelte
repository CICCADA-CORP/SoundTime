<script lang="ts">
  let { value, max, label, valueText, onchange, step = 1, disabled = false }: {
    value: number;
    max: number;
    label: string;
    valueText?: string;
    onchange: (value: number) => void;
    step?: number | "any";
    disabled?: boolean;
  } = $props();

  const filled = $derived(max > 0 ? Math.max(0, Math.min(100, value / max * 100)) : 0);
</script>

<input type="range" min="0" {max} {step} {value} {disabled}
  aria-label={label} aria-valuetext={valueText} style:--filled={filled + "%"}
  oninput={(event) => onchange(event.currentTarget.valueAsNumber)} />

<style>
  input {
    --groove: linear-gradient(to right, var(--slider-fill, hsl(var(--primary))) var(--filled), var(--slider-track, hsl(var(--foreground) / .14)) var(--filled));
    display: block; width: 100%; height: 24px; appearance: none; background: transparent; cursor: pointer;
  }
  input:disabled { cursor: default; opacity: .55; }
  input::-webkit-slider-runnable-track { height: var(--slider-height, 4px); border-radius: 3px; background: var(--groove); }
  input::-moz-range-track { height: var(--slider-height, 4px); border-radius: 3px; background: var(--groove); }
  input::-webkit-slider-thumb { appearance: none; width: var(--slider-thumb-size, 10px); height: var(--slider-thumb-size, 10px); margin-top: calc((var(--slider-height, 4px) - var(--slider-thumb-size, 10px)) / 2); border-radius: 50%; background: var(--slider-fill, hsl(var(--primary))); }
  input::-moz-range-thumb { width: var(--slider-thumb-size, 10px); height: var(--slider-thumb-size, 10px); border: 0; border-radius: 50%; background: var(--slider-fill, hsl(var(--primary))); }
  @media (forced-colors: active) { input { appearance: auto; } }
</style>
