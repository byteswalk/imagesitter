<script setup lang="ts">
import {
  TooltipArrow,
  TooltipContent,
  TooltipPortal,
  TooltipProvider,
  TooltipRoot,
  TooltipTrigger,
} from "reka-ui";

const props = defineProps<{
  text: string;
  side?: "top" | "bottom" | "left" | "right";
}>();
</script>

<template>
  <TooltipProvider :delay-duration="300">
    <TooltipRoot>
      <TooltipTrigger as-child>
        <slot />
      </TooltipTrigger>
      <TooltipPortal>
        <TooltipContent
          :side="props.side ?? 'top'"
          :side-offset="4"
          class="z-50 overflow-hidden rounded-md bg-foreground px-3 py-1.5 text-xs text-background shadow-md"
        >
          {{ props.text }}
          <TooltipArrow class="fill-foreground" />
        </TooltipContent>
      </TooltipPortal>
    </TooltipRoot>
  </TooltipProvider>
</template>
