<script setup lang="ts">
import { TabsRoot, type TabsRootProps } from "reka-ui";

const props = defineProps<TabsRootProps>();
const emit = defineEmits<{ "update:modelValue": [value: string] }>();
</script>

<template>
  <TabsRoot v-bind="props" @update:model-value="emit('update:modelValue', String($event))">
    <slot />
  </TabsRoot>
</template>
