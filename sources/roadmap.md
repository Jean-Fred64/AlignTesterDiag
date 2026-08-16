# AlignTesterDiag — Technical Roadmap & Architecture TODO

Ce document récapitule l'état d'avancement du projet, les acquis validés, ainsi que la feuille de route technique pour le développement des modules de diagnostic avancé, de formatage et de prise en charge multi-systèmes (IBM PC, Atari ST, Amiga, Amstrad CPC).

---

## 📌 1. État d'Avancement Actuel (Current Status)

### ✅ Acquis Validés & Robustesse
- [x] **Architecture Non-Bloquante (~60 Hz) :** Découplage strict entre la boucle de rendu TUI Ratatui/Crossterm et le thread matériel (`try_recv()`, polling d'événements).
- [x] **Support Multi-Lecteurs (26 broches / 34 broches) :** Sélection dynamique d'unité `Drive 0 (A:)` / `Drive 1 (B:)` via la touche `U` et argument CLI `--drive <0|1>`.
- [x] **Motor-Gated Seek :** Réveil moteur synchrone à 15 ms (`STEPPER_WAKEUP_DELAY_MS`) évitant les blocages matériels sur lecteurs slim type TEAC FD-05HG.
- [x] **Timeouts & Capture Greaseweazle :** Utilisation des paramètres nominaux de capture USB (`CMD_READ_FLUX`), tolérance aux retraits de disquette en vol `(0/0 NO DATA / NO DISK)`.
- [x] **Radar Audio de Centrage (Variomètre) :**
  - Thread audio dédié non-bloquant sans latence.
  - Modulation continue de fréquence (440 Hz à 1760 Hz) indexée sur $Q\% = \min(Q_{\text{H0}}, Q_{\text{H1}})$.
  - Alerte dissonante immédiate (220 Hz / Buzz) en cas de piste croisée ($T_{\text{H0}} \ne T_{\text{H1}}$).
  - Clic d'erreur atténué (150 Hz) sur CRC invalide ou secteur manquant.

### 🔄 En Cours de Finalisation
- [ ] **Validation Stricte de Concordance de Piste :** Sanctionner les secteurs hors-piste cible dans le calcul de `Mechanical Alignment` (ex: $T_{\text{H0}}=40$ et $T_{\text{H1}}=41$ sur cible $T=40 \implies 50\%$ d'alignement, ruban orange/rouge).
- [ ] **Fiabilisation du 1er Appui sur 'A' :** Maintien de la boucle d'analyse et spinup moteur garanti (300 ms) dès la première sollicitation.

---

## 🛠️ 2. Module de Diagnostic Avancé & Mécanique (Touche `S`)

- [ ] **Seek Alterné ($0 \leftrightarrow 79$) :** Test d'endurance, de couple maximal et détection des points durs sur la vis sans fin ou la bande métallique.
- [ ] **Random Seek Stress Test :** Sauts pseudo-aléatoires de pistes pour évaluer le temps d'amortissement (*settling time*) et la fidélité de positionnement.
- [ ] **Step Rate Benchmark :** Balayage des cadences d'impulsion de pas (2 ms, 3 ms, 6 ms, 8 ms, 12 ms) pour déterminer le seuil limite de décrochage du moteur pas-à-pas.
- [ ] **Cycle de Nettoyage de Têtes (Head Cleaning) :** Va-et-vient continu et lent ($0 \leftrightarrow 40$) combiné à l'activation moteur pour disquette de nettoyage à l'alcool isopropylique.
- [ ] **Mesure de Jitter de l'Impulsion `/INDEX` :** Analyse de la régularité de rotation et dérive temporelle tour par tour en microsecondes.

---

## 💾 3. Moteur de Formatage Bas Niveau (Low-Level Format - Touche `F`)

- [ ] **Générateur de Piste MFM Standard (PC / Atari ST / Amstrad) :**
  - Synthèse des marques d'index `IAM` (`0xC2`), d'adresses `IDAM` (`0xA1` avec horloge manquante), et de données `DAM` (`0xFB` / `0xF8`).
  - Calcul dynamique du CRC-16 CCITT standard ($x^{16} + x^{12} + x^5 + 1$, init `0xFFFF`).
  - Structuration paramétrable des pauses magnétiques ($Gap_1$, $Gap_2$, $Gap_3$, $Gap_4$) et octets de remplissage configurables (`0xE5`, `0xF6`, `0x00`).
- [ ] **Options de Formatage Interactives :**
  - Piste courante individuelle ou reformatage multipistes complet ($0 \rightarrow 79$).
  - Facteur d'entrelacement (*Interleave* : 1:1, 1:2, 1:3...) et décalage de piste (*Cylinder Skew*).
  - Mode Démagnétisation Totale (*Bulk Erase* sans fronts de flux).

---

## 🕹️ 4. Support Multi-Encodages & Formats Rétro-Informatique

### 🖥️ A. Format Atari ST (WD1772 MFM - 250 kbps)
- [ ] Profils de formats :
  - **Standard :** 9 secteurs / piste (720 Ko double face, 360 Ko simple face).
  - **Overformatted (Fastcopy / Twister) :** 10 et 11 secteurs / piste (800 Ko / 880 Ko).
  - **Pistes Étendues :** Support des pistes 80 à 82.
- [ ] Détection et calcul de conformité adaptés aux $Gap_3$ courts.

### 🦁 B. Format Amiga (Paula MFM - 250 kbps / 500 kbps)
- [ ] **Synchronisation MFM Amiga :** Mot magique `0x44894489` répété deux fois.
- [ ] **Décodage Even/Odd Bits :** Recombinaison des demi-octets pairs et impairs écrits par la puce Paula.
- [ ] **Géométrie AmigaDOS :** 11 secteurs par piste (512 octets/secteur, 880 Ko DD / 1.76 Mo HD).
- [ ] **Intégrité :** Calcul du Checksum Amiga ($XOR$ 32 bits pairs/impairs) à la place du CRC-16 standard.

### 📼 C. Format Amstrad CPC & Lecteurs 3 Pouces (DDI-1 / Schneider)
- [ ] **Numérotation des Secteurs CPC :**
  - Format Data : secteurs `0xC1` à `0xC9` (9 secteurs de 512 octets).
  - Format Système : secteurs `0x41` à `0x49`.
- [ ] **Géométrie 3 Pouces :** 40 pistes par face (support simple/double face réversible).
- [ ] **Gestion des Signaux Spécifiques :** Prise en compte de la ligne `READY` et des spécificités de cadencement des mécanismes Panasonic/Matsushita (EME-156 / EME-216).

---

## 🎛️ 5. Interface Utilisateur TUI & Visualiseur (Touche `I`)

- [ ] **Sélecteur de Profil Machine (Machine Profile) :** Bascule à chaud du format cible (`IBM PC`, `Atari ST`, `Amiga`, `Amstrad CPC`).
- [ ] **Visionneuse Hexadécimale de Secteur (Hex Dump Inspector - Touche `I`) :**
  - Grille 16 colonnes Hex + ASCII pour inspecter le contenu d'un secteur décodé.
  - Détection des marqueurs spéciaux (*Deleted Data*, secteurs protégés, erreurs CRC intentionnelles).