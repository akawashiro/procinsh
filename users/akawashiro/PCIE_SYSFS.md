# PCIe のツリー構造と Linux sysfs への対応

通常の PCIe は、ホスト側の Root Complex を根として、Root Port・スイッチ・Endpoint を接続する階層構造を持つ。Linux は列挙した PCI デバイスの親子関係をデバイスモデルに登録し、sysfs はそれをディレクトリ階層として公開する。そのため、`/sys/bus/pci/devices/` のリンクを実体へ解決すると、上流ブリッジを辿れる。

## 観測環境

以下の PCI アドレス・機器名・sysfs パスは、**masumi で 2026-10-23 に取得した結果**に基づく。ツリー図は `readlink -f` の実体パスを整理したもので、USB・SATA など説明対象外の枝は省略している。

```console
$ hostname
masumi
$ uname -r
7.0.0-38-generic
$ dpkg-query -W -f='${Package} ${Version} ${source:Package} ${source:Version}\n' linux-modules-7.0.0-38-generic
linux-modules-7.0.0-38-generic 7.0.0-38.38 linux 7.0.0-38.38
```

OS は Ubuntu 26.04.1 LTS。`linux-image-7.0.0-38-generic` のソースパッケージ名は `linux-signed` なので、PCI 実装の照合には `linux-modules` が示す `linux` のバージョンを使用する。

## PCIe の構成要素

| 要素 | 役割 |
| --- | --- |
| Root Complex | CPU・メモリ側と PCIe 階層を結ぶホスト側の機構 |
| Root Port | Root Complex から下流の PCIe リンクへ出るポート |
| PCIe Switch | 上流ポートと複数の下流ポートを持ち、接続を分岐させる |
| Endpoint | GPU、NIC、NVMe コントローラーなどの末端デバイス |

各外部リンクはポイントツーポイント接続で、スイッチを介して枝分かれする。Root Complex ごとの通常の階層をツリーとして扱える。

根拠は [Renesas / IDT AN-510](https://www.renesas.com/document/510-use-non-transpared-bridging-idt-pci-express-ntb-gen1-switches) の Introduction・Figure 1 を参照する。

## 物理的なスイッチと OS から見えるブリッジ

一つの PCIe スイッチは、ソフトウェアからは複数の PCI-to-PCI Bridge と内部の仮想 PCI バスとして見える。上流ポートと各下流ポートがブリッジ機能を持つため、**一つのスイッチを通る経路でも、sysfs 上では複数のブリッジ階層になる**。この対応は AN-510 の Figure 1 の挿入図に示されている。

masumi の NVMe SSD は、AMD 500 Series Chipset の上流ポートと下流ポートを経由する。一方、NVIDIA GPU は別の GPP bridge の直下にある。実体パスをまとめると次のようになる。

```text
/sys/devices/
└─ pci0000:00/                 Linux の PCI host bridge オブジェクト
   ├─ 0000:00:01.2/            AMD Starship/Matisse GPP Bridge
   │  └─ 0000:01:00.2/         AMD 500 Series Chipset Switch Upstream Port
   │     ├─ 0000:02:04.0/      同 Switch Downstream Port
   │     │  └─ 0000:03:00.0/   SanDisk / WD Blue SN570 NVMe SSD
   │     └─ 0000:02:08.0/      同 Switch Downstream Port
   │        └─ 0000:04:00.0/   Realtek RTL8125 Ethernet
   └─ 0000:00:03.1/            AMD Starship/Matisse GPP Bridge
      ├─ 0000:05:00.0/         NVIDIA GeForce RTX 5060 Ti（VGA function）
      └─ 0000:05:00.1/         NVIDIA GB206（Audio function）
```

機器名は `lspci -D -nn` の表示を使っている。関連する出力の抜粋は次のとおり。

```text
0000:01:00.2 PCI bridge [0604]: Advanced Micro Devices, Inc. [AMD] 500 Series Chipset Switch Upstream Port [1022:43e9]
0000:02:04.0 PCI bridge [0604]: Advanced Micro Devices, Inc. [AMD] 500 Series Chipset Switch Downstream Port [1022:43ea]
0000:02:08.0 PCI bridge [0604]: Advanced Micro Devices, Inc. [AMD] 500 Series Chipset Switch Downstream Port [1022:43ea]
0000:03:00.0 Non-Volatile memory controller [0108]: Sandisk Corp SanDisk Ultra 3D / WD Blue SN570 NVMe SSD (DRAM-less) [15b7:501a]
0000:04:00.0 Ethernet controller [0200]: Realtek Semiconductor Co., Ltd. RTL8125 2.5GbE Controller [10ec:8125] (rev 05)
0000:05:00.0 VGA compatible controller [0300]: NVIDIA Corporation GB206 [GeForce RTX 5060 Ti] [10de:2d04] (rev a1)
0000:05:00.1 Audio device [0403]: NVIDIA Corporation GB206 High Definition Audio Controller [10de:22eb] (rev a1)
```

ここで、ディレクトリ間の辺すべてが独立した物理 PCIe リンクを表すわけではない。スイッチ内部の仮想バスを介する親子関係も含まれる。ブリッジのディレクトリ数をそのまま物理スイッチ数として数えることもできない。

`0000:03:00.0` は `domain:bus:device.function` 形式の PCI アドレスである。末端の単位は PCI function なので、一枚のカードに複数のエントリが存在することもある。`pci0000:00` は host bridge オブジェクトの名前で、PCI function のアドレスではない。これを CPU ソケットや Root Complex 全体と一対一に対応すると仮定しない。

**パス文字列から分かる親子関係だけで、ポートや機器の種別がすべて確定するわけではない。** 上の名前は `lspci` の PCI ID データベースによる表示であり、PCI Express Capability のポート種別を直接確認した結果とは区別する。masumi の一般ユーザーでの `lspci -vv` は `Capabilities: <access denied>` となるため、図の `00:01.2` と `00:03.1` も観測できた GPP Bridge という名前で記載している。

## なぜ sysfs の階層から親子関係が分かるのか

公式文書と Linux の実装を、次の順に組み合わせると確認できる。ソースコードは **Linus の `torvalds/linux` ツリーの Linux v7.0** を参照し、GitHub のリンクをコミット `028ef9c96e96197026887c0f092424679298aae8` に固定する。

masumi の `7.0.0-38-generic` は Ubuntu ソースバージョン `7.0.0-38.38` で、対応する Makefile は `VERSION = 7`・`PATCHLEVEL = 0`・`SUBLEVEL = 14`（Linux 7.0.14 ベース）。7.0 系の stable 更新を含む実機との差を明確にするため、Linus ツリー側ではその基点の [v7.0 リリースコミット](https://github.com/torvalds/linux/commit/028ef9c96e96197026887c0f092424679298aae8) を使用する。以下の `probe.c` の3箇所は、実機と一致する Ubuntu ソースパッケージとも照合し、コードと行番号が一致することを確認している。

1. **sysfs はオブジェクトの親子関係をディレクトリにする。**
   [sysfs 文書の Directory Creation](https://docs.kernel.org/filesystems/sysfs.html#directory-creation) は、登録された kobject のディレクトリを、その親 kobject のサブディレクトリとして作成すると説明している。同文書の devices の説明では、`/sys/devices` が `struct device` の階層に直接対応するとしている。

2. **PCI デバイスの親は、所属バスの上流ブリッジになる。**
   [`pci_setup_device()`（`drivers/pci/probe.c:2028`）](https://github.com/torvalds/linux/blob/028ef9c96e96197026887c0f092424679298aae8/drivers/pci/probe.c#L2028) は `dev->dev.parent = dev->bus->bridge;` と設定する。

3. **ブリッジ配下のバスは、そのブリッジを保持する。**
   [`pci_alloc_child_bus()`（`drivers/pci/probe.c:1241`）](https://github.com/torvalds/linux/blob/028ef9c96e96197026887c0f092424679298aae8/drivers/pci/probe.c#L1241) は `child->bridge = get_device(&bridge->dev);` と設定する。その子バスに列挙されたデバイスには、上の処理でこのブリッジが親として設定される。

4. **根の host bridge には `pci<domain>:<bus>` という名前が付く。**
   [`pci_register_host_bridge()`（`drivers/pci/probe.c:1028`）](https://github.com/torvalds/linux/blob/028ef9c96e96197026887c0f092424679298aae8/drivers/pci/probe.c#L1028) で `dev_set_name(&bridge->dev, "pci%04x:%02x", ...)` と命名している。

この対応が、上流ブリッジからデバイスまでの階層が sysfs の実体パスに現れる根拠となる。特定のバス番号や階層数を固定する仕様ではない。

## `/sys/bus/pci/devices` は一覧、`/sys/devices` は階層

`/sys/bus/pci/devices/` は PCI デバイスを列挙する入口で、各エントリは `/sys/devices/` 以下へのシンボリックリンクになっている。

```text
/sys/bus/pci/devices/0000:03:00.0
  → /sys/devices/pci0000:00/0000:00:01.2/0000:01:00.2/0000:02:04.0/0000:03:00.0
```

[sysfs のアクセス規則](https://www.kernel.org/doc/html/latest/admin-guide/sysfs-rules.html) の devpath と「Hierarchy in a single device tree」は、シンボリックリンクを実体へ解決し、階層を調べる際は `/sys/devices` 配下を使うよう説明している。

取得手順は次のようになる。

1. `/sys/bus/pci/devices/` のエントリを列挙する。
2. 各リンクを実体パスへ解決する。
3. 実体ディレクトリから祖先を辿る。PCI デバイスかどうかは各ディレクトリの `subsystem` リンクなどで確認する。
4. 各 PCI function と上流ブリッジの対応を保存し、複数の経路に現れる共通の祖先をまとめる。
5. 種別や属性は各デバイス自身から読む。親の属性を子の属性として扱わない。

プラットフォームによっては host bridge より上にもディレクトリがある。パスの固定位置や固定の深さ、アドレスの大小関係から親子関係を推測しない。ホットプラグ等で走査中にデバイスが消える可能性も考慮する。

## 実機で確認するコマンド

次は masumi で実行したコマンドと出力。NVMe SSD の経路にはスイッチの上流・下流ポートが含まれ、GPU の経路には含まれない。

```console
$ readlink -f /sys/bus/pci/devices/0000:03:00.0
/sys/devices/pci0000:00/0000:00:01.2/0000:01:00.2/0000:02:04.0/0000:03:00.0
$ readlink -f /sys/bus/pci/devices/0000:04:00.0
/sys/devices/pci0000:00/0000:00:01.2/0000:01:00.2/0000:02:08.0/0000:04:00.0
$ readlink -f /sys/bus/pci/devices/0000:05:00.0
/sys/devices/pci0000:00/0000:00:03.1/0000:05:00.0
$ cat /sys/bus/pci/devices/0000:03:00.0/class
0x010802
$ cat /sys/bus/pci/devices/0000:03:00.0/vendor
0x15b7
$ cat /sys/bus/pci/devices/0000:03:00.0/device
0x501a
```

`lspci -D -t` でもこの枝を照合できる。次はその出力の関連部分を抜粋したもの（間の無関係な行は省略）。`[01-04]` などはブリッジ配下のバス範囲で、末端の `00.0` はそのバス上の device/function を表す。

```text
           +-01.2-[01-04]--+-00.0
           |               +-00.1
           |               \-00.2-[02-04]--+-04.0-[03]----00.0
           |                               \-08.0-[04]----00.0
           +-03.1-[05]--+-00.0
           |            \-00.1
```

属性の説明は [Accessing PCI device resources through sysfs](https://docs.kernel.org/PCI/sysfs-pci.html) と [PCI sysfs ABI 定義](https://www.kernel.org/doc/Documentation/ABI/testing/sysfs-bus-pci) を参照する。前者には host bridge 配下の PCI function と属性ファイルの例があるが、masumi の構成そのものを掲載した文書ではない。

このツリーはカーネルが列挙する PCI 階層であり、プロセスがどのデバイスを開いているか、転送量がどれくらいかを表すものではない。それらは FD や活動情報を別途観測して対応付ける。
