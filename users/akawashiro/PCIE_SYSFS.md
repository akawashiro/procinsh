# PCIe のツリー構造と Linux sysfs への対応

通常の PCIe は、ホスト側の Root Complex を根として、Root Port・スイッチ・Endpoint を接続する階層構造を持つ。Linux は列挙した PCI デバイスの親子関係をデバイスモデルに登録し、sysfs はそれをディレクトリ階層として公開する。そのため、`/sys/bus/pci/devices/` のリンクを実体へ解決すると、上流ブリッジを辿れる。

以下の図と PCI アドレスは**説明用の例**であり、特定の実機の観測結果や仕様書からの転載ではない。

## PCIe の構成要素

| 要素 | 役割 |
| --- | --- |
| Root Complex | CPU・メモリ側と PCIe 階層を結ぶホスト側の機構 |
| Root Port | Root Complex から下流の PCIe リンクへ出るポート |
| PCIe Switch | 上流ポートと複数の下流ポートを持ち、接続を分岐させる |
| Endpoint | GPU、NIC、NVMe コントローラーなどの末端デバイス |

```text
Root Complex
├─ Root Port ─ GPU
└─ Root Port ─ PCIe Switch
               ├─ NVMe SSD
               └─ PCIe Switch
                    ├─ GPU
                    └─ NIC
```

各外部リンクはポイントツーポイント接続で、スイッチを介して枝分かれする。Root Complex ごとの通常の階層をツリーとして扱えるが、マシン全体に根が一つしかないという意味ではない。NTB（Non-Transparent Bridge）では、独立したアドレス空間を持つ PCIe 階層同士を接続できる。

根拠は [Renesas / IDT AN-510](https://www.renesas.com/document/510-use-non-transpared-bridging-idt-pci-express-ntb-gen1-switches) の Introduction・Figure 1 と、続く NTB の説明。リンクの性質や複数ホスト構成については [Microchip の PCIe Interconnect Architecture 解説](https://ww1.microchip.com/downloads/aemDocuments/documents/UNG/ProductDocuments/SupportingCollateral/PCIe-White-Paper-DS00005141.pdf) も参照できる。

## 物理的なスイッチと OS から見えるブリッジ

一つの PCIe スイッチは、ソフトウェアからは複数の PCI-to-PCI Bridge と内部の仮想 PCI バスとして見える。上流ポートと各下流ポートがブリッジ機能を持つため、**一つのスイッチを通る経路でも、sysfs 上では複数のブリッジ階層になる**。この対応は AN-510 の Figure 1 の挿入図に示されている。

例えば次の物理構成を考える。

```text
Root Port ─ Switch の上流ポート
             └─ Switch 内部 ─ 下流ポート ─ GPU
```

その列挙結果を、次のような sysfs 階層で表せる。

```text
/sys/devices/
└─ pci0000:00/                Linux の PCI host bridge オブジェクト
   └─ 0000:00:01.0/           Root Port
      └─ 0000:01:00.0/        Switch の上流ポート
         └─ 0000:02:00.0/     同じ Switch の下流ポート
            └─ 0000:03:00.0/  GPU の PCI function
```

ここで、ディレクトリ間の辺すべてが独立した物理 PCIe リンクを表すわけではない。スイッチ内部の仮想バスを介する親子関係も含まれる。ブリッジのディレクトリ数をそのまま物理スイッチ数として数えることもできない。

`0000:03:00.0` は `domain:bus:device.function` 形式の PCI アドレスである。末端の単位は PCI function なので、一枚のカードに複数のエントリが存在することもある。`pci0000:00` は host bridge オブジェクトの名前で、PCI function のアドレスではない。これを CPU ソケットや Root Complex 全体と一対一に対応すると仮定しない。

また、この図の Root Port・上流ポート・下流ポート・GPU という種別は、例として与えたもの。**パス文字列から分かる親子関係だけで、これらの種別がすべて確定するわけではない。** PCI class や PCI Express Capability のポート種別など、別の情報が必要になる。

## なぜ sysfs の階層から親子関係が分かるのか

公式文書と Linux の実装を、次の順に組み合わせると確認できる。実装へのリンクは参照位置が動かないよう Linux **v6.12** に固定している。

1. **sysfs はオブジェクトの親子関係をディレクトリにする。**
   [sysfs 文書の Directory Creation](https://docs.kernel.org/filesystems/sysfs.html#directory-creation) は、登録された kobject のディレクトリを、その親 kobject のサブディレクトリとして作成すると説明している。同文書の devices の説明では、`/sys/devices` が `struct device` の階層に直接対応するとしている。

2. **PCI デバイスの親は、所属バスの上流ブリッジになる。**
   [`pci_setup_device()`](https://github.com/torvalds/linux/blob/v6.12/drivers/pci/probe.c#L1910) は `dev->dev.parent = dev->bus->bridge;` と設定する。

3. **ブリッジ配下のバスは、そのブリッジを保持する。**
   [`pci_alloc_child_bus()`](https://github.com/torvalds/linux/blob/v6.12/drivers/pci/probe.c#L1150) は `child->bridge = get_device(&bridge->dev);` と設定する。その子バスに列挙されたデバイスには、上の処理でこのブリッジが親として設定される。

4. **根の host bridge には `pci<domain>:<bus>` という名前が付く。**
   [`pci_register_host_bridge()`](https://github.com/torvalds/linux/blob/v6.12/drivers/pci/probe.c#L943) で `dev_set_name(&bridge->dev, "pci%04x:%02x", ...)` と命名している。

この対応が、上流ブリッジからデバイスまでの階層が sysfs の実体パスに現れる根拠となる。特定のバス番号や階層数を固定する仕様ではない。

## `/sys/bus/pci/devices` は一覧、`/sys/devices` は階層

`/sys/bus/pci/devices/` は PCI デバイスを列挙する入口で、各エントリは `/sys/devices/` 以下へのシンボリックリンクになっている。

```text
/sys/bus/pci/devices/0000:03:00.0
  → /sys/devices/pci0000:00/0000:00:01.0/0000:01:00.0/0000:02:00.0/0000:03:00.0
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

次のコマンドはリンクと属性を読むだけで、デバイス設定を書き換えない。アドレスは実機に存在するものに置き換える。

```sh
# PCI function の一覧
ls /sys/bus/pci/devices/

# 対象の実体パス（上流ブリッジの階層を含む）
readlink -f /sys/bus/pci/devices/0000:03:00.0

# 対象自身の属性
cat /sys/bus/pci/devices/0000:03:00.0/class
cat /sys/bus/pci/devices/0000:03:00.0/vendor
cat /sys/bus/pci/devices/0000:03:00.0/device

# pciutils がある場合、列挙された階層と照合する
lspci -D -t
```

属性の説明は [Accessing PCI device resources through sysfs](https://docs.kernel.org/PCI/sysfs-pci.html) と [PCI sysfs ABI 定義](https://www.kernel.org/doc/Documentation/ABI/testing/sysfs-bus-pci) を参照する。前者には host bridge 配下の PCI function と属性ファイルの例があるが、上記の多段スイッチ構成そのものを掲載した文書ではない。

このツリーはカーネルが列挙する PCI 階層であり、プロセスがどのデバイスを開いているか、転送量がどれくらいかを表すものではない。それらは FD や活動情報を別途観測して対応付ける。
