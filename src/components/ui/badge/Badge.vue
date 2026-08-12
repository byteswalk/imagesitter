<script setup lang="ts">
import type { HTMLAttributes } from "vue";
import { cn } from "@/lib/utils";

const props = defineProps<{
  class?: HTMLAttributes["class"];
  variant?: "default" | "secondary" | "destructive" | "outline" | "success";
}>();

const variantClasses: Record<string, string> = {
  default: "border-transparent bg-primary text-primary-foreground",
  secondary: "border-transparent bg-secondary text-secondary-foreground",
  destructive:
    "border-transparent bg-destructive text-destructive-foreground",
  success: "border-transparent bg-emerald-600 text-white",
  outline: "text-foreground",
};
</script>

<template>
  <span
    :class="
      cn(
        'inline-flex items-center rounded-md border px-2 py-0.5 text-xs font-semibold transition-colors',
        variantClasses[props.variant ?? 'default'],
        props.class,
      )
    "
  >
    <slot />
  </span>
</template>
