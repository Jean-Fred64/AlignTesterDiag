# Documentation Technique - Système d'Alignement ImageDisk

## Vue d'ensemble

Le système d'alignement d'ImageDisk est une fonctionnalité critique permettant de tester et d'ajuster la position de la tête de lecture/écriture d'un lecteur de disquette. Il permet de détecter les problèmes d'alignement mécanique et de diagnostiquer les erreurs de lecture/écriture.

## Architecture générale

### Fonction principale : `align()`

**Localisation** : `IMD.C`, lignes 2143-2312

La fonction `align()` est le point d'entrée principal du système d'alignement. Elle fournit une interface interactive permettant de :

- Analyser les pistes du disque
- Détecter les problèmes d'alignement
- Tester la lecture/écriture de secteurs
- Formater des pistes
- Ajuster manuellement la position de la tête

### Structure de données clés

```c
// Variables globales utilisées pour l'alignement
unsigned char
    Cylinder,      // Cylindre actuel (0-79)
    Head,          // Tête actuelle (0 ou 1)
    Sector,        // Secteur actuel
    Resync,        // Compteur de resynchronisation
    Drive;         // Lecteur sélectionné (0-3)

unsigned
    Result[15],    // Résultats du contrôleur FDC
    SEG,           // Segment DMA aligné sur 64k
    Timeout;       // Délai d'attente

struct SIDE {
    unsigned Stop;              // Nombre de secteurs
    unsigned char Smap[MSEC];   // Carte de numérotation des secteurs
    unsigned char Cmap[MSEC];   // Carte des cylindres
    unsigned char Hmap[MSEC];   // Carte des têtes
} S0, S1, *S;
```

## Mécanismes d'alignement

### 1. Détection d'alignement en temps réel

**Localisation** : `IMD.C`, lignes 2188-2206

Le système utilise une boucle continue qui lit les IDs de secteurs passant sous la tête :

```c
for(;;) {
    if(readid()) {
        // Échec de lecture - réinitialiser
        wprintf("\n?");
        memset(sm, y = n = df = 0, sizeof(sm));
    }
    else {
        t = Result[3];  // Cylindre détecté
        s = Result[5] & 0x1F;  // Numéro de secteur
        
        // Comptage des secteurs détectés
        if(sm[s] < 2) {
            ++sm[s];
            if(t == ct)  // Cylindre attendu
                ++y;       // Compteur de succès
            else
                ++n;       // Compteur d'erreurs
        }
        else {
            // Affichage des résultats
            wprintf(" %u %u\n", y, n);
            if(!bf)
                beep(500+(y*100), 100);  // Signal sonore
            memset(sm, y=n=0, sizeof(sm));
        }
    }
}
```

**Algorithme de détection** :
1. Lecture continue des IDs de secteurs via `readid()`
2. Comparaison du cylindre détecté (`Result[3]`) avec le cylindre attendu (`ct`)
3. Comptage des secteurs correctement alignés (`y`) vs mal alignés (`n`)
4. Signal sonore proportionnel au nombre de secteurs corrects

### 2. Fonction `readid()` - Lecture d'ID de secteur

**Localisation** : `IMD.C`, lignes 1089-1093

```c
int readid(void)
{
    wrcmd(0x0A);  // Commande "Read ID" du FDC
    return result(7);  // Lit jusqu'à 7 octets de résultat
}
```

**Fonctionnement** :
- Envoie la commande 0x0A au contrôleur FDC (Floppy Disk Controller)
- Lit les résultats dans `Result[]` :
  - `Result[0]` : Status Register 0 (ST0)
  - `Result[1]` : Status Register 1 (ST1)
  - `Result[2]` : Status Register 2 (ST2)
  - `Result[3]` : **Cylindre détecté**
  - `Result[4]` : **Tête détectée**
  - `Result[5]` : **Numéro de secteur**
  - `Result[6]` : **Taille du secteur**

**Codes de retour** :
- `0` : Succès, ID lu correctement
- `0xC0` ou `0xD8` : Erreur (bits d'erreur dans ST0)

### 3. Fonction `resync()` - Resynchronisation de la tête

**Localisation** : `IMD.C`, lignes 1050-1084

Cette fonction implémente une stratégie progressive de récupération en cas d'erreur :

```c
void resync(void)
{
    unsigned c = Cylinder;
    switch(++Resync) {
    case 1 : 
        Cstat('E');  // Affiche 'E' pour erreur
        return;
        
    case 2 :  // 1ère tentative - Step in/out
    case 5 :
        Cstat('1');
        if(c) {
            seek(c-1); delay(200);  // Reculer d'un cylindre
            seek(c);   delay(200);   // Revenir
        }
        return;
        
    case 3 :  // 2ème tentative - Step out/in
    case 6 :
        Cstat('2');
        if(c < Cylinders) {
            seek(c+1); delay(200);  // Avancer d'un cylindre
            seek(c);   delay(200); // Revenir
        }
        return;
        
    case 4 :  // 3ème tentative - Recalibration
    case 7 :
        Cstat('3');
        recal(); delay(250);  // Recalibrer à la piste 0
        seek(c);
        return;
        
    case 8 :  // 4ème tentative - Réinitialisation complète
        Cstat('4');
        deselect_drive();
        delay(1000);
        out(FDC, DORsel[Drive]);
        delay(500);
        initmode();
        Resync = 1;
    }
}
```

**Stratégie de récupération** :
1. **Tentative 1** : Affiche l'erreur
2. **Tentatives 2/5** : Mouvement in/out (recalage mécanique)
3. **Tentatives 3/6** : Mouvement out/in (recalage inverse)
4. **Tentatives 4/7** : Recalibration complète (retour à la piste 0)
5. **Tentative 8** : Réinitialisation du contrôleur FDC

### 4. Fonction `seek()` - Positionnement de la tête

**Localisation** : `IMD.C`, lignes 1027-1045

```c
void seek(unsigned c)
{
    Cylinder = c;
    
    // Vérification d'interruption utilisateur
    if(c = kbtst()) {
        if((Lkey = c) == 0x1B)
            xabort();
    }
    
    delay(55);
    wrfdc(0x0F);  // Commande "Seek"
    wrfdc(Drive); // ID du lecteur
    out(FDC, DORsel[Drive] | 0x08);  // Activer IRQ
    
    // Attendre l'interruption
    waitirq(Dds ? Cylinder+Cylinder : Cylinder);
    
    // Vérifier que le seek est terminé
    do {
        delay(55);
        wrfdc(0x08);  // "Sense Interrupt Status"
        result(2);
    }
    while(in(FDCS) & 0x0F);  // Tant que le seek est en cours
    
    if(SKdel)
        delay(SKdel);  // Délai supplémentaire si configuré
}
```

**Fonctionnement** :
- Envoie la commande Seek (0x0F) au FDC
- Attend l'interruption matérielle du FDC
- Vérifie le statut jusqu'à ce que le mouvement soit terminé
- Gère le double-step si activé (`Dds`)

### 5. Fonction `analyze_track()` - Analyse complète d'une piste

**Localisation** : `IMD.C`, lignes 1191-1291

Cette fonction analyse une piste complète pour déterminer :
- Le nombre de secteurs
- La numérotation des secteurs
- L'interleave
- Les paramètres de format

**Algorithme principal** :

```c
// Phase 1 : Synchronisation sur le trou d'index
// Trouve un mode d'accès invalide pour s'aligner sur une frontière de secteur
for(;;) {
    initmode();
    if(readid())  // Échec = on est sur le trou d'index
        break;
    adjmode(0);   // Essayer le mode suivant
}

// Phase 2 : Lecture des IDs de secteurs
i = 0;
X1: adjmode(0);
    initmode();
    Resync = f = 0;
    
    for(;;) {
        if(readid()) {
            // Gestion des erreurs avec retry
            if(++i < 12) goto X1;
            return 255;
        }
        
        // Extraction des informations
        cy = Result[3];  // Cylindre
        hd = Result[4];  // Tête
        s  = Result[5];  // Secteur
        n  = Result[6];  // Taille
        
        // Détection de secteurs dupliqués
        if(Sflag[s]++) {
            switch(Sflag[s]) {
            case 4 :
                if(((h-l)+1) == c)
                    goto X2;  // Analyse complète
                continue;
            case 20 : f = "Missing sectors"; goto X2;
            case 8 :
            case 12 :
            case 16 : resync();  // Resynchronisation
            }
            continue;
        }
        
        // Ajout du secteur à la carte
        Cmap[Stop] = cy;
        Hmap[Stop] = hd;
        Smap[Stop++] = s;
        
        // Mise à jour des bornes
        if(s < l) l = s;
        if(s > h) h = s;
        ++c;
    }
```

**Points clés** :
- Synchronisation initiale sur le trou d'index
- Détection de secteurs manquants ou dupliqués
- Gestion automatique de la resynchronisation
- Calcul de l'interleave basé sur la séquence des secteurs

## Interface utilisateur d'alignement

### Commandes disponibles

La fonction `align()` propose les commandes suivantes :

| Commande | Fonction |
|----------|----------|
| `A` | Analyser la piste actuelle |
| `B` | Activer/désactiver le signal sonore |
| `D` | Lire les données de tous les secteurs |
| `F` | Formater la piste actuelle |
| `H` | Basculer entre tête 0 et 1 |
| `I` | Sauvegarder une image de la piste |
| `P` | Configurer les paramètres de format |
| `R` | Recalibrer et repositionner |
| `S` | Basculer entre single/double step |
| `W` | Écrire des données de test |
| `Z` | Recalibrer à la piste 0 |
| `0-9` | Se positionner sur la piste (0-90) |
| `+/-` | Se déplacer d'une piste |
| `X` | Quitter le mode alignement |

### Affichage en temps réel

**Ligne de statut** (lignes 2212-2219) :
- Affiche les flags de statut du FDC :
  - `F/f` : Format
  - `W/w` : Write
  - `R/r` : Read
  - `Z/z` : Zero
  - `D/d` : Data

**Affichage de détection** (lignes 2193-2206) :
- Format : `[cylindre] [secteur] [succès] [erreurs]`
- Exemple : `43 5 8 0` = Cylindre 43, Secteur 5, 8 succès, 0 erreur

## Mécanismes de bas niveau

### Contrôleur FDC (Floppy Disk Controller)

**Adresse de base** : `FDC = 0x3F2` (par défaut)
- `FDCS = FDC+2` : Registre de statut
- `FDCD = FDC+3` : Registre de données

**Commandes utilisées** :
- `0x03` : Specify (configuration)
- `0x04` : Sense Drive Status
- `0x05` : Write Data
- `0x06` : Read Data
- `0x07` : Recalibrate
- `0x08` : Sense Interrupt Status
- `0x0A` : **Read ID** (critique pour l'alignement)
- `0x0D` : Format Track
- `0x0F` : Seek

### Gestion DMA (Direct Memory Access)

**Segment aligné** : `SEG` aligné sur une frontière 32k
```c
SEG = (SEG + 0x7FF) & 0xF800;  // Alignement sur 32k
```

**Initialisation DMA** (lignes 897-911) :
```c
void initdma(unsigned char mode, unsigned count)
{
    disable();
    out(0x0A, 6);  // Masquer le canal
    out(FDC, DORsel[Drive] | 0x08);  // Activer DMA & IRQ
    out(0x0B, mode);  // Mode (0x46=read, 0x4A=write)
    out(0x0C, 0);  // Réinitialiser pointeur
    out(0x81, DMA >> 12);  // Page DMA
    out(0x04, (DMA << 4) & 255);  // Adresse basse
    out(0x04, (DMA >> 4) & 255);  // Adresse haute
    out(0x05, --count);  // Compte bas
    out(0x05, count >> 8);  // Compte haut
    out(0x0A, 2);  // Démasquer canal
    enable();
}
```

### Gestion des interruptions

**Vecteur d'interruption** : IRQ 6 (INT 0x0E)

**Handler d'interruption** (lignes 723-731) :
```c
asm {
XFDONE DB 0  ; Flag 'DONE'
FINT:   PUSH AX
        MOV  AL,20h
        OUT  20h,AL  ; Envoyer EOI au PIC
        MOV  CS:XFDONE,255  ; Définir flag
        POP  AX
        IRET
}
```

**Attente d'interruption** (lignes 852-878) :
- Attend jusqu'à 75 ticks BIOS (~4 secondes)
- Vérifie le flag `XFDONE` après chaque commande FDC

## Détection et diagnostic d'alignement

### Indicateurs d'alignement

1. **Compteur de succès (`y`)** : Nombre de secteurs détectés sur le bon cylindre
2. **Compteur d'erreurs (`n`)** : Nombre de secteurs détectés sur un mauvais cylindre
3. **Signal sonore** : Fréquence proportionnelle au nombre de succès (500 + y*100 Hz)

### Interprétation des résultats

- **`y` élevé, `n` faible** : Alignement correct
- **`y` faible, `n` élevé** : Problème d'alignement mécanique
- **`y` et `n` faibles** : Problème de format ou de disque
- **Lecture impossible** : Problème matériel ou disque non formaté

### Stratégies de correction

1. **Ajustement manuel** : Utiliser `+/-` pour déplacer la tête
2. **Recalibration** : Commande `R` ou `Z` pour recalibrer
3. **Resynchronisation automatique** : Le système tente automatiquement via `resync()`
4. **Test de format** : Commande `F` pour reformater la piste

## Paramètres configurables

### Variables globales affectant l'alignement

```c
unsigned char
    SKdel = 55,      // Délai après seek (ms)
    Retry = 5,       // Nombre de tentatives
    Dstep = 0x55,    // Double-step (0x55=auto, 0=off, 0xFF=on)
    Dana = 255,      // Analyse détaillée (0=enable, 255=disable)
    Timeout = 36;    // Timeout FDC (en ticks BIOS, ~2s)

unsigned
    SPsrt = 8,       // Taux de step (ms)
    SPhlt = 127,     // Temps de chargement de tête (ms)
    SPhut = 15;      // Temps de déchargement de tête (ms)
```

### Options de ligne de commande

- `/S[adresse]` : Adresse du FDC secondaire
- `SK=[valeur]` : Délai après seek
- `SR=[valeur]` : Taux de step
- `HL=[valeur]` : Temps de chargement de tête
- `HU=[valeur]` : Temps de déchargement de tête

## Limitations et considérations

### Limitations matérielles

1. **Accès direct au matériel** : Nécessite DOS pur (ne fonctionne pas sous Windows NT/2000/XP)
2. **Contrôleur FDC 765** : Compatible uniquement avec les contrôleurs compatibles NEC 765
3. **DMA 16 bits** : Nécessite un segment aligné sur 32k

### Limitations logicielles

1. **Taille maximale de secteur** : 256 secteurs par piste (`MSEC = 256`)
2. **Cylindres maximum** : 255 (configurable)
3. **Drives maximum** : 4 (A, B, C, D)

### Points d'attention

1. **Timing critique** : Les délais sont cruciaux pour le fonctionnement correct
2. **Gestion d'erreurs** : Le système doit gérer les timeouts et les erreurs matérielles
3. **Interruptions** : La gestion des interruptions doit être précise

## Exemples d'utilisation

### Test d'alignement basique

1. Lancer ImageDisk
2. Sélectionner `A` (Alignment/test)
3. Insérer une disquette formatée
4. Observer les valeurs `y` et `n` dans l'affichage
5. Utiliser `+/-` pour ajuster si nécessaire

### Diagnostic de problème

1. Si `n > 0` : Problème d'alignement mécanique
   - Solution : Ajuster manuellement ou utiliser `R` pour recalibrer
2. Si lecture impossible : Vérifier le format du disque
   - Solution : Utiliser `A` pour analyser, puis `F` pour formater si nécessaire
3. Si erreurs intermittentes : Problème de timing
   - Solution : Ajuster `SKdel`, `SPsrt`, `SPhlt`, `SPhut`

## Références techniques

### Registres FDC

- **Status Register 0 (ST0)** : État général
  - Bit 7 : Interrupt Code
  - Bit 6 : Seek End
  - Bit 5 : Equipment Check
  - Bit 4 : Not Ready
  - Bit 3 : Head Address
  - Bits 2-0 : Unit Select

- **Status Register 1 (ST1)** : Erreurs de lecture
  - Bit 7 : End of Cylinder
  - Bit 6 : Data Error
  - Bit 5 : Overrun
  - Bit 4 : No Data
  - Bit 2 : Not Writable
  - Bit 1 : Missing Address Mark

- **Status Register 2 (ST2)** : Erreurs de données
  - Bit 7 : Missing Address Mark in Data Field
  - Bit 6 : Bad Cylinder
  - Bit 5 : Scan Not Satisfied
  - Bit 4 : Scan Equal Hit
  - Bit 3 : Wrong Cylinder
  - Bit 2 : Data Error in Data Field
  - Bit 1 : Control Mark

### Formats de disquette supportés

- **FM (Frequency Modulation)** : Simple densité
- **MFM (Modified Frequency Modulation)** : Double densité
- **Taux de données** : 500, 300, 250 kbps
- **Tailles de secteur** : 128, 256, 512, 1024, 2048, 4096, 8192 octets

## Conclusion

Le système d'alignement d'ImageDisk est un outil sophistiqué permettant de diagnostiquer et corriger les problèmes d'alignement mécanique des lecteurs de disquettes. Il combine des mécanismes de détection en temps réel, des stratégies de récupération automatique, et une interface utilisateur interactive pour un contrôle précis.

La compréhension de ces mécanismes est essentielle pour :
- Développer des outils compatibles
- Adapter le système à d'autres plateformes
- Diagnostiquer des problèmes matériels
- Optimiser les performances de lecture/écriture

