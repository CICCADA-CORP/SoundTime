<script lang="ts">
  let { mood, gradient, emoji, onclick }: {
    mood: string;
    gradient: string;
    emoji: string;
    onclick?: () => void;
  } = $props();
</script>

<button
  class="relative rounded-xl overflow-hidden h-28 w-full group cursor-pointer text-left transition-transform hover:scale-[1.02]"
  {onclick}
>
  <div class="absolute inset-0 {gradient} opacity-90 group-hover:opacity-100 transition-opacity"></div>
  <div class="relative h-full flex flex-col justify-end p-4">
    <span class="text-2xl mb-1">{emoji}</span>
    <span class="text-sm font-bold text-white">{mood}</span>
  </div>
</button>
