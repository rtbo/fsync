<script>
  import { goto } from '$app/navigation';
  import { daemonInstanceName } from '$lib/model';

  daemonInstanceName()
    .then(async (name) => {
      if (!name || name === '') {
        await goto('/connect');
      } else {
        await goto('/nav/' + name);
      }
    })
    .catch(async (err) => {
      console.error(err);
      await goto('/connect');
    });
</script>

<div></div>
