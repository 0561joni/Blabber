# Lokale Übersetzung: Messung und Abnahmestand

Stand: 18. September 2026. Die Funktion ist implementiert und als **Preview**
gekennzeichnet. Die technische Prüfung auf diesem Mac ist erfolgreich; die
sprachliche Freigabe und die Abnahme mit echtem Mikrofon und Zielanwendungen sind
noch offen. Insbesondere sind französische Grammatik- und Anredefehler bekannt.
Die geforderte menschliche Erfolgsquote von mindestens 95 % ist nicht nachgewiesen.

## Prüfgegenstand

- Apple M3 Pro, 18 GiB Arbeitsspeicher (19.327.352.832 Bytes), Metal.
- TranslateGemma 12B Q6_K, 9.660.827.392 Bytes; Modellrevision
  `1076826a801dbc6cc8ad4ff4689a3272dcb8a378`.
- Modell-SHA-256:
  `c30995b3c145e6ef3b3a6fda63749186d83d9c8f16725ff1403c904b5e0ead8c`.
- llama.cpp `972d2313bc0bf0a45f634f77d95c9fb03aeab12c`, Protokoll 1, Prompt 4.
- SHA-256 des geprüften und gebündelten Helpers:
  `12e972687c35220e382c62e2948028a6a9da518f6742b3c2946ed6867ffab66c`.
- Das Modell wurde tatsächlich heruntergeladen und seine Prüfsumme geprüft.
  Modell und synthetische Testdaten wurden nicht in die persönlichen
  Einstellungen oder den persönlichen Verlauf installiert.

Architektur, Schnittstellen und Reproduktion stehen in
[docs/translation.md](../docs/translation.md).

## Automatisierte Prüfung und Paket

| Prüfung | Ergebnis |
| --- | --- |
| Frontend, Vitest | 100 Tests in 21 Dateien bestanden |
| Rust-Bibliothek | 164 bestanden, 7 bestehende native Prüfungen ignoriert |
| Abschließende gezielte Rust-Prüfung nach letzter Zustandsanpassung | 8 bestanden |
| `cargo check --all-targets` | Bestanden |
| `npm run build` | Bestanden |
| `npm run tauri -- build --bundles app` | Apple-Silicon-App gebaut |
| Native Übersetzung mit gebündeltem Helper | Erfolgreich |
| Ungültiges Protokoll und fehlerhaftes JSON | Strukturierte Fehler, kein Echo des Eingabetexts |
| `codesign --verify --deep --strict` | Bestanden, lokale Ad-hoc-Signatur |
| DMG-Paket | Erstellt, rund 20 MB ohne Modellgewichte |

Die Tests decken unter anderem den Sprachzyklus, Sitzungsbindung, Abbruch,
Warteschlangenbesitz, Protokollvalidierung, Verlaufsmigration, Wiederherstellung
unterbrochener Übersetzungen, Löschweitergabe, Export und Ergebnisoberfläche ab.
Die Oberfläche wurde zusätzlich im Browser geprüft; dies ersetzt keine native
Hotkey- oder Einfügeprüfung.

Der Helper und die Gemma-/llama.cpp-Lizenzdateien sind im App-Paket enthalten.
Der Helper benötigt keine separat installierte Modell-Runtime. Das Paket ist
lokal signiert, jedoch nicht mit Developer ID signiert oder notarisiert.

Die vorhandenen Command Line Tools ermöglichten den nativen Build. Der
Build-Wrapper wählt sie auf diesem Mac automatisch, sofern `DEVELOPER_DIR` nicht
explizit gesetzt wurde. Die Xcode-Lizenz und die globale Werkzeugauswahl wurden
nicht verändert.

## Übersetzungskorpus

[Rohdaten des endgültigen Laufs](translation-12b-q6-m3-pro.jsonl): 160 eindeutige
Ergebnisse, 40 je Sprachrichtung. Alle Datensätze verwenden exakt den oben
genannten Helper, das Modell und Prompt 4. Bei der Wiederaufnahme wurden
abweichende Helper-Versionen erneut ausgeführt.

**160/160 automatische Prüfungen bestanden.** Geprüft wurden vollständiger
Prozessabschluss, nicht leere Ausgabe, ausgewählte unverändert zu übernehmende
Textstellen, unerwünschte Vorreden sowie mehrteilige Texte und Absatzgrenzen.
Diese Prüfungen belegen weder vollständige Bedeutungstreue noch korrekte
Negationen, Anrede oder argentinischen Sprachgebrauch. Alle Felder für menschliche
Bewertung sind deshalb weiterhin unbelegt.

| Richtung | Median gesamt | 95. Perzentil | Median Modellladen | Langer Text |
| --- | ---: | ---: | ---: | ---: |
| DE → FR | 4,236 s | 5,132 s | 0,761 s | 102,641 s |
| DE → ES-AR | 3,997 s | 5,030 s | 0,763 s | 103,781 s |
| EN → FR | 4,419 s | 5,718 s | 0,764 s | 113,419 s |
| EN → ES-AR | 4,043 s | 4,922 s | 0,765 s | 88,318 s |

Gesamt bedeutet hier nur Start bis Ende des Übersetzungsprozesses, einschließlich
Modellladen; Aufnahme, ASR und Einfügen sind nicht enthalten. Das 95. Perzentil
verwendet den nächsthöheren Rang bei 40 Werten. Pro Richtung ist ein langer Text
mit 16 Absätzen enthalten: jeweils zwei Quelltextabschnitte, sämtliche Absätze
erhalten. Die übrigen Beispiele sind überwiegend kurze Nachrichten.

Das gemessene Laden lag zwischen 0,729 und 1,624 Sekunden. Jeder Auftrag startete
einen neuen Prozess; wiederholter Zugriff auf dieselbe Modelldatei profitiert
jedoch vom Dateicache des Betriebssystems. Das ist keine Zusage für einen kalten
App-Start oder das Verhalten unter anderer Systemlast.

### Bekannte sprachliche Abweichungen

Diese Stichproben sind Hinweise aus der technischen Sichtung, keine vollständige
menschliche Abnahme:

| Fall | Beobachtung |
| --- | --- |
| `dictation-01-de-fr` | „Kannst du mir bitte morgen die Datei schicken?“ wird zu „Peux-tu me envoyer le fichier demain, s'il te plaît ?“. Die Elision zu „m'envoyer“ fehlt. |
| `dictation-12-de-fr` | „Vielen Dank für Ihre Rückmeldung. Ich werde Ihre Anfrage morgen beantworten.“ wird zu „Merci beaucoup pour ton retour. Je répondrai à ta demande demain.“ Die formelle Anrede geht verloren. |
| `dictation-13-en-fr` | „Please let us know whether you will be able to attend the appointment.“ wird zu „Veuillez nous faire savoir si tu pourras assister au rendez-vous.“ Die Anrede ist uneinheitlich. |

Diese Fehler bestehen auch mit Prompt 4 und verhindern eine uneingeschränkte
Qualitätszusage. Weitere Prompt-/Modellanpassungen brauchen einen erneuten
Korpuslauf und eine sprachkundige Bewertung. Es wurde kein kleineres Modell als
Fallback eingebaut.

[Prompt-1-Rohdaten](translation-12b-q6-m3-pro-prompt1.jsonl) bleiben als historische
Vergleichsdaten erhalten. Sie enthalten frühere Fehler und eine kürzere Fassung
des Langtextfalls und zählen nicht zur endgültigen Abnahme.

## Speicher und wiederholte Verarbeitung

Der maximale Prozess-RSS im endgültigen Korpus betrug **11.410.800.640 Bytes
(10,63 GiB)**. Der Bereich lag zwischen etwa 10,57 und 10,63 GiB. Der Median der
ersten bzw. letzten 20 kurzen Ergebnisse in der finalen Messfolge war
11.409.326.080 bzw. 11.408.883.712 Bytes. Dabei ist kein steigender Prozess-RSS
erkennbar. Da jeder Helper beendet wird, ist dies keine Langzeitprüfung aller
Speicherbereiche des laufenden Elternprozesses.

[Systemmessung](translation-memory-pressure-m3-pro.json): 569 Stichproben in rund
19 Minuten im Abstand von etwa zwei Sekunden während eines Teils der
Korpusausführung. 533 meldeten normalen Speicherdruck, 36 eine Warnstufe; keine
Stichprobe meldete kritischen Speicherdruck. Der Messabschnitt erfasst nicht alle
Neustarts und Wiederholungsläufe lückenlos.

Swap war bereits vor Beginn belegt: 4.433,38 MB zu Beginn und 4.875,38 MB am Ende,
mit einem beobachteten Maximum von 5.115,50 MB. Andere Anwendungen liefen parallel.
Es lässt sich daher weder ein Betrieb ohne Swap noch eine alleinige Verursachung
des Swap-Anstiegs durch Blabber behaupten. Prozess-RSS ist außerdem nicht mit dem
gesamten Unified-Memory-Bedarf des Systems gleichzusetzen.

## Gebündelte ASR- und Übersetzungskette

[Rohdaten](translation-pipeline-m3-pro.json): drei nacheinander ausgeführte
Prozesspaare aus dem fertig gebündelten Programm, jeweils ASR vollständig beenden
und anschließend Übersetzung starten. Verwendet wurden Whisper Small mit Metal
und eine lokal synthetisierte deutsche Aufnahme von 9,108 Sekunden:

> Kannst du mir bitte morgen die Datei schicken? Ich brauche nicht die alte
> Version, sondern die neue. Der Preis beträgt 125 Euro.

| Durchlauf | Ziel | ASR | Übersetzung | Zusammen |
| --- | --- | ---: | ---: | ---: |
| 1 | FR | 11,996 s | 25,987 s | 37,983 s |
| 2 | ES-AR | 11,486 s | 7,896 s | 19,382 s |
| 3 | FR | 11,605 s | 9,678 s | 21,283 s |

Alle Prozesse endeten erfolgreich. Die ASR erkannte den Inhalt mit einer
abweichenden Satzgrenze. Die französische Ausgabe enthält weiterhin den oben
beschriebenen Elisionsfehler; die spanische Ausgabe verwendet „¿Podés enviarme…?“.
Diese drei Stichproben ersetzen keine Sprachabnahme. Die stärker schwankenden
Zeiten zeigen, dass die kurzen isolierten Korpustests keine Zusage für die
vollständige Diktierlatenz darstellen. Die Aufnahmedauer ist in „Zusammen“ nicht
enthalten.

Die ASR-Prozesse erreichten rund 0,80 GiB RSS, die anschließenden
Übersetzungsprozesse 7,91 bis 10,34 GiB. Niedrigerer residenter Speicher allein
belegt keinen geringeren gesamten Speicherbedarf. Diese Prüfung umfasst keine
echte Mikrofonaufnahme, keine Hotkey-Ereignisse und keinen Einfügevorgang.

## Vor einer regulären Freigabe offen

1. Alle 160 Beispiele durch Menschen bewerten, mindestens 95 % je Richtung ohne
   notwendige inhaltliche Korrektur. Kritische Fehler bei Zahlen, Negationen oder
   Anweisungen müssen ausgeschlossen werden. Französisch und argentinische
   Anredeformen benötigen sprachkundige Prüfung.
2. Echte globale und manuelle Diktate prüfen: beide Hotkeys, Tastaturwiederholung,
   Fokus, Zwischenablage, Berechtigungen, genau einmaliges Einfügen, Abbruch und
   Shutdown unmittelbar vor dem Einfügen sowie erneute Übersetzung ohne Autopaste.
3. Modellinstallation mit unterbrochenem Download, beschädigter Datei und
   anschließender Reparatur in der gebündelten Oberfläche durchspielen. Den
   Offline-Betrieb nach Installation praktisch prüfen.
4. Wiederholte vollständige Diktate mit dem tatsächlich bevorzugten ASR-Modell
   und den üblichen Anwendungen messen; dabei auch den Elternprozess und
   dauerhaften Speicherdruck beobachten.
5. Regression der unveränderten Datei-Transkription in der gebündelten App,
   Builds der anderen Plattformen sowie Developer-ID-Signierung und Notarisierung
   vor öffentlicher Verteilung durchführen.

Die Preview kann lokal getestet werden. Diese offenen Punkte werden nicht als
bestandene Abnahme dargestellt.
