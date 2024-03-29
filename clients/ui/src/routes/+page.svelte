<script>
  import { invoke } from '@tauri-apps/api/tauri';
  import { goto } from '$app/navigation';

  invoke('daemon_connected').then((connected) => {
    if (!connected) {
      goto('connect');
    }
  });
</script>

<div></div>
