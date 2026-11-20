# Desktop engine candidates

ScarletNX is the initial Windows/Linux adapter. No alternative reviewed provided convincing
primary-source evidence of broader, reliable game coverage with a simpler integration contract.
This is a practical integration choice, not a benchmark or a claim of verified compatibility.
Only native preparation/runtime engines are candidates; another frontend is not an engine.

| Candidate | Relevant support and limits | Integration decision |
| --- | --- | --- |
| [ScarletNX](https://github.com/scarletnx/ScarletNX) | Windows/Linux x86-64 packages, Vulkan, NSP/XCI extraction, separate updates, native game output and an explicit frozen-runtime launch mode. No named supported-game matrix; reports use anonymous game IDs. Nightlies have limited retention. | Experimental first adapter. Broad Switch 1 title listings are unverified. Keep packages and each game's runtime pinned. |
| [Kirikae](https://github.com/Cor3Duo/kirikae) | Native x86-64 recompilation. Inspected v0.1.2 release exposes a Windows asset; Linux support exists in source. Decrypted base games only, with updates/DLC unsupported. | Revisit as Linux packaging and input support mature. Not evidence of wider verified coverage. |
| [NativeNX](https://github.com/Smithy-Lab/NativeNX) | Windows ZIP and Linux AppImage, extracted inputs, AVX2/Vulkan. The README explicitly marks the project unmaintained and Windows as not tested on real hardware by its author. | Poorer maintenance basis for the first adapter. |
| [nxlinux](https://github.com/luckberonne/nxlinux) | Linux source project focused on Mario Kart 8 Deluxe 4.0.1; no published releases in the inspected repository, with audio/movie work remaining. | Potential specialized route, not a general coverage replacement. |
| [SwitchRECOMP](https://github.com/SwitchRECOMP-Dev/SwitchRECOMP) | Windows/Linux artifacts; Python/compiler dependencies, extracted game inputs and platform-specific graphics paths. No broad verified game matrix found. | Keep as a future adapter candidate; does not establish better coverage. |
| [Switch2RECOMP](https://github.com/SwitchRECOMP-Dev/Switch2RECOMP) | Claims Switch 1/2 and Windows/Linux targets. v0.4.1 reports include [Linux runtime hangs](https://github.com/SwitchRECOMP-Dev/Switch2RECOMP/issues/1), [Mario Kart World startup failure](https://github.com/SwitchRECOMP-Dev/Switch2RECOMP/issues/4) and [Vulkan rendering failure](https://github.com/SwitchRECOMP-Dev/Switch2RECOMP/issues/5). Additional compiler/shader dependencies; documented Linux setup includes a global memory-policy change. | Promising future investigation, especially for Switch 2; insufficient evidence to replace the first adapter. Do not automatically change host security settings. |

These observations describe the inspected repository/release contracts, not permanent rankings.
Reassess from upstream source, release assets and named game evidence when considering another
adapter. Different engines can coexist through [the same contract](ADDING-ENGINES.md), with
separate system/target declarations, catalogs and per-game pins. Never convert a project's
claimed target support into a claim that every catalog title is playable.

## Other console pipelines

The admission rule is a generic game preparation route or ready profiles maintained in the engine
repository. Adding another game must not require a developer to write a title-specific port in
RecompLauncher. Host build fixes are separate from game-specific recompilation work.

| Candidate | Integration decision |
| --- | --- |
| [ModernGekko](https://github.com/ExpansionPak/ModernGekko), with DolRecomp/RecompCore | Generic GameCube/Wii disc extraction and module build; Apple silicon adapter implemented. Other host routes are not advertised until packaged and validated. |
| [ps2-native-recomp](https://github.com/exoteric33/ps2-native-recomp) | Generic ISO pipeline and upstream game reports; Apple silicon adapter implemented. The runtime's live compiler/SDK stays frozen with each game. |
| [ps3recomp](https://github.com/sp00nznet/ps3recomp/tree/de4f090328ea08bef6b2f15caec0b85e343a7ece) | Deferred. The inspected SDK's examples require title-specific harnesses and runtime SPU capture/relifting; there is no ready generic install pipeline or complete shared profile catalog. No installable placeholder is exposed. |
| [PS4Recomp](https://github.com/wie-project/PS4Recomp) | Generic decrypted-folder-to-standalone-app pipeline; Apple silicon adapter implemented with pinned upstream compatibility reports. |
| [AnyPS5](https://github.com/boykopovar/AnyPS5) | Generic decrypted ELF relinker/runtime; Windows/Linux x86-64 adapter implemented. Linux needs glibc 2.43. Bring-up reports do not certify the pinned release, and actual target testing remains required. |

Each implemented adapter keeps its verified artifacts, importer, recipe, maintenance guide and
tests in its own folder. No compatibility claim follows simply from inclusion in this table.
