# Dictation scripts for the ASR benchmark

Read these aloud to build the test corpus for `blabber-bench` (see [the benchmark plan](asr-benchmark-plan.md)). Each script has an ID. Name each recording after its ID, and the benchmark will pick up the script text as the starting reference automatically.

The machine-readable source is [`asr-benchmark-scripts.json`](asr-benchmark-scripts.json). Edit texts there, not here.

## Overview

| Set | Clips | Audio |
|---|---|---|
| German: short dictations | 15 | ~2 min 57 s |
| English: short dictations | 15 | ~2 min 48 s |
| Mixed language: short dictations | 10 | ~88 s |
| French: short dictations | 6 | ~47 s |
| Long recordings | 6 | ~18 min 38 s |
| No speech (hallucination check) | 3 | ~60 s |
| Condition re-recordings | 13 | ~3 min 19 s |
| **Total** | **68** | **about 31 min** |

Plan about an hour including retakes. You can record in several sessions. German and English come first; French and the condition re-recordings are optional extras.

## How to record

- **Microphone:** use the one you normally dictate with, in your normal position.
- **App:** QuickTime Player → File → New Audio Recording. Pick the microphone in the menu next to the record button and set quality to *Maximum*. Voice Memos works too. Any format is fine (m4a, wav); the benchmark converts it.
- **File names:** use the script ID, e.g. `de-s05.m4a`. Condition re-recordings use `@`, e.g. `de-s05@noisy.m4a`. Put everything in one folder.
- **Silence:** leave about one second of silence before you start and after you finish.
- **Style:** speak as you would when dictating: natural, not performed. Read the short scripts as a single dictation each.
- **Numbers:** read them the way you would normally say them: "14 Uhr" → *vierzehn Uhr*, "8,5" → *acht Komma fünf*, "3 pm" → *three p m*, "Q1" → *Q eins*. Don't change the wording (say *vierzehn Uhr*, not *zwei Uhr nachmittags*).
- **Punctuation:** do not say commas or full stops.
- **Mistakes:** small slips are fine; keep going. The review step corrects the reference to what you actually said. Re-record only if you lose the thread.
- **Mixed-language scripts:** switch language exactly where the text switches. The language markers (*DE*, *EN*, *FR*) are only for you; do not read them.

## German: short dictations

15 clips, ~2 min 57 s in total.

### `de-s01`

<sub>~2 s · very-short</sub>

> Ja, passt so.

### `de-s02`

<sub>~3 s · very-short</sub>

> Bitte ruf mich morgen früh zurück.

### `de-s03`

<sub>~10 s · email, names, numbers</sub>

> Hallo Frau Özdemir, vielen Dank für Ihre schnelle Rückmeldung. Ich schicke Ihnen das überarbeitete Angebot bis Donnerstag um 14 Uhr.

### `de-s04`

<sub>~12 s · business, numbers</sub>

> Im dritten Quartal ist der Umsatz um 12 Prozent gestiegen, aber die EBITDA-Marge liegt mit 8,5 Prozent noch unter unserem Ziel von 11 Prozent.

### `de-s05`

<sub>~10 s · engineering, software, denglisch</sub>

> Der Pull Request für die Datenbankmigration ist fertig. Kannst du bitte das Review übernehmen, bevor wir heute Abend auf Staging deployen?

### `de-s06`

<sub>~12 s · engineering, hardware, numbers</sub>

> Laut Lastenheft muss das Gehäuse eine Toleranz von plus minus 0,05 Millimetern einhalten. Die erste Spritzgussserie hat das bei drei von zwanzig Teilen nicht geschafft.

### `de-s07`

<sub>~8 s · names</sub>

> Für den Workshop in München haben zugesagt: Herr Schäfer, Frau Dr. Weißgerber, Krzysztof Nowak und Siobhan Gallagher.

### `de-s08`

<sub>~8 s · numbers</sub>

> Die Rechnung über 3.480 Euro ist am 15. November fällig. Bitte überweise den Betrag bis spätestens Freitag.

### `de-s09`

<sub>~17 s · business, list, denglisch</sub>

> Kurze Notiz zum Jour fixe: Erstens, die Roadmap für Q1 wird bis Ende des Monats finalisiert. Zweitens, wir brauchen eine Entscheidung zum Make-or-Buy beim Sensormodul. Drittens, das nächste Steering Committee ist am 3. Dezember.

### `de-s10`

<sub>~14 s · engineering, software, numbers</sub>

> Seit dem letzten Release liegt die Latenz der API im p95 bei über 800 Millisekunden. Ich schlage einen Rollback auf Version 2.3 vor, bis wir die Ursache gefunden haben.

### `de-s11`

<sub>~8 s · casual</sub>

> Bin gleich da, stehe noch im Stau auf der A9. Fangt ruhig schon ohne mich an.

### `de-s12`

<sub>~12 s · umlauts, engineering</sub>

> Wegen der geänderten Maße an der Übergabestelle müssen wir die Prüfstände in Göttingen und Fürth neu kalibrieren. Das verzögert die Abnahme um etwa zwei Wochen.

### `de-s13`

<sub>~42 s · email, business, numbers, near-60s</sub>

> Hallo Thomas, kurzes Update zum Kundenprojekt bei der Hansen Logistik GmbH. Das Kick-off am Dienstag lief gut, der Kunde war mit dem Zeitplan einverstanden. Allerdings möchten sie die Schnittstelle zu ihrem SAP-System schon in der ersten Projektphase haben und nicht erst im März. Das bedeutet für uns etwa zwanzig zusätzliche Personentage. Ich würde vorschlagen, dass wir dafür einen Change Request mit einem Festpreis von 18.500 Euro anbieten. Kannst du mir bis morgen Mittag sagen, ob wir die Kapazität im Team haben? Danke und viele Grüße.

### `de-s14`

<sub>~13 s · business, finance, legal, denglisch</sub>

> Bevor wir das Term Sheet unterschreiben, sollte die Due Diligence abgeschlossen sein. Insbesondere die Pensionsrückstellungen und die Change-of-Control-Klauseln in den Lieferverträgen müssen wir uns genauer ansehen.

### `de-s15`

<sub>~6 s · compounds</sub>

> Die Kundenzufriedenheitsumfrage hat ergeben, dass die Reaktionszeit unseres Kundendienstes das größte Ärgernis ist.

## English: short dictations

15 clips, ~2 min 48 s in total.

### `en-s01`

<sub>~1 s · very-short</sub>

> Sounds good, thanks.

### `en-s02`

<sub>~4 s · very-short</sub>

> Can you send me the slides before the call?

### `en-s03`

<sub>~11 s · email, names, numbers</sub>

> Hi Priya, thanks for the quick turnaround. I've attached the revised statement of work, and I'd like to get sign-off by Thursday at 3 pm.

### `en-s04`

<sub>~11 s · business, numbers</sub>

> Our annual recurring revenue grew to 4.2 million dollars, but net revenue retention dropped to 96 percent because churn in the mid-market segment picked up.

### `en-s05`

<sub>~10 s · engineering, software</sub>

> The Kubernetes cluster ran out of memory during the nightly batch job, so the pods kept getting evicted and the CI pipeline timed out.

### `en-s06`

<sub>~14 s · engineering, software, numbers</sub>

> Let's add a feature flag around the new checkout flow and roll it out to 5 percent of users first. If the error rate stays below 0.1 percent, we ramp up on Monday.

### `en-s07`

<sub>~7 s · names</sub>

> Please loop in Siobhan Gallagher, Nguyen Van Minh, and Joaquín Herrera from the procurement team.

### `en-s08`

<sub>~7 s · numbers</sub>

> The purchase order for 1,250 units at 18.40 dollars each comes to 23,000 dollars, not including shipping.

### `en-s09`

<sub>~12 s · engineering, list, names</sub>

> Action items from today's standup: Marcus will fix the flaky integration test, Elena owns the OAuth token refresh bug, and I'll write the postmortem for Tuesday's outage.

### `en-s10`

<sub>~13 s · engineering, hardware, numbers</sub>

> The thermal simulation shows the heat sink is undersized. At full load, the junction temperature hits 105 degrees Celsius, which is right at the limit of the spec sheet.

### `en-s11`

<sub>~4 s · casual</sub>

> Running ten minutes late, the train is stuck outside Paddington.

### `en-s12`

<sub>~10 s · business, software</sub>

> For the Q2 roadmap, I'd prioritize single sign-on and audit logs, because three enterprise prospects listed them as blockers in the last RFP.

### `en-s13`

<sub>~44 s · engineering, data, numbers, near-60s</sub>

> Quick status update on the data platform migration. We've moved 70 percent of the batch jobs from the old Hadoop cluster to the new Spark setup on Databricks, and the nightly runtime dropped from six hours to just under two. The remaining jobs are the ones with complex dependencies on legacy stored procedures, so they'll take longer. We also found a schema drift issue in the customer table that was silently dropping records with null postal codes. That's fixed now, and we've added a data quality check to the pipeline. Next milestone is decommissioning the old cluster by the end of November.

### `en-s14`

<sub>~11 s · business, finance</sub>

> The board wants a revised forecast with EBITDA, free cash flow, and a sensitivity analysis on the exchange rate, ideally before the offsite in October.

### `en-s15`

<sub>~9 s · homophones</sub>

> Their team said they're going to review the two proposals over there, and the lead engineer will lead the review too.

## Mixed language: short dictations

10 clips, ~88 s in total.

### `mx-s01`

<sub>~8 s · DE / EN · sentence-switch, numbers</sub>

> *DE* Ich bin heute bis 16 Uhr im Büro.  
> *EN* After that, I'm only reachable on my phone.

### `mx-s02`

<sub>~7 s · EN / DE · sentence-switch, engineering</sub>

> *EN* Can you check the build logs?  
> *DE* Ich glaube, der Fehler kommt von der neuen Abhängigkeit.

### `mx-s03`

<sub>~9 s · DE · intra-sentence, denglisch, business</sub>

> *DE* Lass uns das im nächsten Weekly kurz syncen, die Deadline für das Pitch Deck ist ja schon am Freitag.

### `mx-s04`

<sub>~7 s · DE · intra-sentence, denglisch, engineering</sub>

> *DE* Der Merge-Konflikt kommt vom Refactoring im Auth-Service, ich mache einen Rebase und pushe danach nochmal.

### `mx-s05`

<sub>~16 s · EN / DE · sentence-switch, business</sub>

> *EN* Good morning everyone.  
> *DE* Heute geht es um die Budgetplanung für nächstes Jahr.  
> *EN* The main question is whether we hire two more engineers or outsource the mobile app.  
> *DE* Ich bin eher für die interne Lösung.

### `mx-s06`

<sub>~10 s · EN / DE · sentence-switch, legal, numbers</sub>

> *EN* I'll forward you the contract.  
> *DE* Bitte prüf vor allem Paragraf 7 zur Haftung.  
> *EN* Let me know if anything looks off.

### `mx-s07`

<sub>~7 s · DE / FR · sentence-switch, french</sub>

> *DE* Ich schicke dir gleich die Unterlagen.  
> *FR* Merci beaucoup pour ton aide, on se parle demain.

### `mx-s08`

<sub>~7 s · EN / FR · sentence-switch, french</sub>

> *EN* The supplier in Lyon confirmed the delivery.  
> *FR* Ils livreront les pièces mardi prochain, avant midi.

### `mx-s09`

<sub>~7 s · DE · names, intra-sentence</sub>

> *DE* Kannst du Jean-Baptiste Moreau von Airbus und Emily Clarke von Rolls-Royce zum Kick-off einladen?

### `mx-s10`

<sub>~10 s · DE / EN · sentence-switch, rapid-switch, engineering</sub>

> *DE* Okay, kurzer Status.  
> *EN* The API migration is done.  
> *DE* Die Tests laufen alle grün.  
> *EN* Next step is the load test on Wednesday.

## French: short dictations

6 clips, ~47 s in total.

### `fr-s01`

<sub>~2 s · very-short</sub>

> D'accord, ça marche pour moi.

### `fr-s02`

<sub>~9 s · email, names, numbers</sub>

> Bonjour Madame Lefèvre, je vous remercie pour votre retour. Je vous envoie le devis révisé d'ici jeudi à 14 heures.

### `fr-s03`

<sub>~9 s · business, numbers</sub>

> Le chiffre d'affaires du troisième trimestre a augmenté de 7 pour cent, mais la marge brute reste inférieure à nos prévisions.

### `fr-s04`

<sub>~12 s · engineering, software</sub>

> Le déploiement de la nouvelle version a échoué à cause d'une erreur dans la migration de la base de données. Nous allons faire un retour arrière ce soir.

### `fr-s05`

<sub>~7 s · numbers</sub>

> La commande porte sur 2 500 pièces à 3,20 euros l'unité, soit 8 000 euros hors taxes.

### `fr-s06`

<sub>~8 s · names, business</sub>

> Pour la réunion de lancement, j'ai invité Benoît Girard, Amélie Chevalier et le responsable des achats de Valeo.

## Long recordings

6 clips, ~18 min 38 s in total.

### `de-l01`

<sub>~3 min 22 s · business, engineering, hardware, numbers, names</sub>

> Guten Morgen zusammen, hier ist der Statusbericht für das Projekt Nordlicht, Stand Kalenderwoche 41.
>
> Zuerst die gute Nachricht: Der Prototyp der neuen Steuereinheit hat die Umweltprüfung bestanden. Wir haben die Platine über 72 Stunden bei minus 20 bis plus 85 Grad Celsius getestet, und es gab keinen einzigen Ausfall. Damit ist der Meilenstein B-Muster formal erreicht, zwei Wochen früher als im Projektplan vorgesehen.
>
> Weniger erfreulich ist die Situation beim Lieferanten für die Leistungshalbleiter. Die Lieferzeit hat sich von 16 auf 26 Wochen verlängert, und der Stückpreis steigt ab Januar um 9 Prozent. Herr Schäfer aus dem Einkauf verhandelt gerade mit einem zweiten Lieferanten in Taiwan. Ein Angebot erwarten wir bis Ende nächster Woche. Falls das nicht klappt, müssen wir das Layout anpassen, damit wir auf ein pin-kompatibles Bauteil ausweichen können. Das würde etwa 40.000 Euro zusätzlich kosten und den Serienanlauf um sechs Wochen verschieben.
>
> Auf der Softwareseite läuft es weitgehend nach Plan. Die Firmware ist auf Version 1.8 gestiegen, und die automatisierten Regressionstests decken inzwischen 83 Prozent des Codes ab. Offen sind noch zwei kritische Bugs: Erstens verliert der CAN-Bus nach einem Warmstart gelegentlich die Verbindung, und zweitens schlägt das Over-the-Air-Update fehl, wenn die Mobilfunkverbindung während des Downloads abbricht. Für beide Fehler gibt es bereits einen Lösungsansatz, und Frau Dr. Weißgerber rechnet damit, dass die Fixes bis zum 24. Oktober im Release-Branch sind.
>
> Beim Thema funktionale Sicherheit sind wir etwas im Verzug. Die FMEA für das Gesamtsystem ist zu etwa 70 Prozent fertig, aber für die Bewertung nach ISO 26262 fehlen uns noch Daten zur Ausfallrate der Sensorik. Ich schlage vor, dass wir dafür kurzfristig einen externen Gutachter hinzuziehen. Die Kosten dafür liegen bei ungefähr 15.000 Euro und sind im Risikobudget bereits eingeplant.
>
> Zum Budget insgesamt: Wir haben bisher 1,2 Millionen Euro von geplanten 1,75 Millionen ausgegeben. Das entspricht 69 Prozent bei einem Projektfortschritt von etwa 65 Prozent. Wir liegen also leicht über dem Plan, aber noch im grünen Bereich. Wenn der zweite Lieferant nicht zustande kommt, wird das Budget allerdings knapp.
>
> Die nächsten Schritte sind folgende: Bis Freitag entscheiden wir, ob wir das Layout vorsorglich anpassen. Am 30. Oktober findet das Design Review mit dem Kunden in Stuttgart statt, und dafür brauchen wir bis spätestens 27. Oktober die aktualisierte Stückliste und die Testberichte. Außerdem möchte ich im nächsten Lenkungskreis über eine mögliche Verschiebung des Serienstarts sprechen, damit wir keine Überraschungen erleben.
>
> Wenn es Fragen gibt, meldet euch gern direkt bei mir. Vielen Dank.

### `de-l02`

<sub>~3 min 17 s · engineering, software, cloud, numbers, denglisch</sub>

> Ich möchte kurz das technische Konzept für die Migration unserer Auftragsverwaltung in die Cloud zusammenfassen.
>
> Heute läuft das System auf drei physischen Servern in unserem Rechenzentrum in Frankfurt. Die Anwendung ist ein Monolith in Java, die Daten liegen in einer Oracle-Datenbank mit ungefähr 2,3 Terabyte. Die größten Probleme sind die langen Release-Zyklen, die fehlende Skalierbarkeit zum Monatsende und die hohen Lizenzkosten.
>
> Unser Ziel ist eine Architektur mit Containern auf einem verwalteten Kubernetes-Cluster. Wir wollen den Monolithen nicht auf einmal neu schreiben, sondern Schritt für Schritt einzelne Funktionen herauslösen. Man nennt dieses Vorgehen auch das Strangler-Fig-Pattern. Als Erstes nehmen wir uns die Rechnungsstellung vor, weil sie klar abgegrenzt ist und die meiste Last verursacht.
>
> Für die Datenbank planen wir einen Wechsel auf PostgreSQL. Die Migration erfolgt in drei Phasen. In der ersten Phase replizieren wir die Daten mit Change Data Capture fortlaufend in die neue Datenbank. In der zweiten Phase lesen die neuen Services bereits aus PostgreSQL, während die Schreibzugriffe noch auf Oracle laufen. Erst in der dritten Phase schalten wir die Schreibzugriffe um und nehmen die alte Datenbank außer Betrieb.
>
> Ein wichtiger Punkt ist der Datenschutz. Da wir personenbezogene Kundendaten verarbeiten, muss das Rechenzentrum des Cloud-Anbieters in der Europäischen Union liegen, und wir brauchen einen Auftragsverarbeitungsvertrag nach DSGVO. Außerdem werden alle Daten im Ruhezustand und bei der Übertragung verschlüsselt. Die Schlüssel verwalten wir selbst über ein Hardware-Sicherheitsmodul.
>
> Für den Betrieb setzen wir auf Infrastructure as Code mit Terraform. Jede Änderung an der Infrastruktur läuft durch dieselbe CI/CD-Pipeline wie der Anwendungscode, inklusive Code-Review und automatisierter Tests. Für das Monitoring verwenden wir Prometheus und Grafana, und wir definieren für jeden Service klare Service Level Objectives. Für die Rechnungsstellung zum Beispiel soll die Antwortzeit im 95. Perzentil unter 300 Millisekunden liegen, bei einer Verfügbarkeit von 99,9 Prozent.
>
> Zu den Kosten: Die laufenden Infrastrukturkosten schätzen wir auf etwa 18.000 Euro pro Monat. Das ist zunächst mehr als heute, aber durch den Wegfall der Oracle-Lizenzen sparen wir ab dem zweiten Jahr rund 250.000 Euro jährlich. Die einmaligen Migrationskosten liegen bei ungefähr 600.000 Euro, vor allem für interne Entwicklerkapazität und externe Beratung.
>
> Die größten Risiken sehe ich bei der Datenkonsistenz während der Übergangsphase und beim Know-how im Team. Deshalb planen wir eine zweiwöchige Schulung zu Kubernetes und Observability, bevor die eigentliche Migration beginnt. Wenn alles nach Plan läuft, ist die Rechnungsstellung bis Ende des zweiten Quartals vollständig in der Cloud.

### `en-l01`

<sub>~3 min 20 s · business, finance, numbers</sub>

> Here's a summary of our third quarter business review.
>
> Overall, it was a solid quarter. Revenue came in at 12.4 million dollars, which is 6 percent above plan and 18 percent higher than the same quarter last year. Gross margin improved from 61 to 64 percent, mainly because we moved our hosting to reserved instances and renegotiated the contract with our payment processor.
>
> New business was strong in the enterprise segment. We closed 14 new enterprise accounts, including two Fortune 500 companies in logistics. The average contract value went up to 86,000 dollars, and the sales cycle shortened from 120 to about 95 days. Most of that improvement came from the new proof-of-concept program, where prospects get a two-week sandbox with their own data.
>
> The mid-market segment was weaker. Churn increased to 2.8 percent per month, and the main reasons in exit interviews were pricing and missing integrations with accounting software. Net revenue retention across all segments dropped slightly to 108 percent. To address this, the product team has moved the QuickBooks and Xero integrations up the roadmap, and customer success is launching a health score that flags at-risk accounts 60 days before renewal.
>
> On the cost side, operating expenses were 3 percent over budget. The main driver was hiring: we brought on 22 people this quarter, mostly engineers and solutions architects, and recruiting fees were higher than expected. Marketing spend was on target, but the cost per qualified lead went up by roughly 15 percent, so we're shifting budget from paid search to partner programs and events.
>
> Cash flow remains healthy. We ended the quarter with 31 million dollars in cash and generated positive free cash flow for the second quarter in a row. EBITDA margin was 9 percent, compared with minus 4 percent a year ago.
>
> Looking ahead to the fourth quarter, we have three priorities. First, hit our bookings target of 15 million dollars, which requires closing at least four of the six large deals currently in late stage. Second, reduce mid-market churn below 2 percent by the end of the year. Third, finish the SOC 2 Type II audit, because several European prospects have made it a hard requirement, along with GDPR documentation and a data processing agreement.
>
> There are two risks I want to flag. The first is currency: about 30 percent of our revenue is billed in euros and pounds, and a stronger dollar would hurt reported revenue. The second is the timing of the large deals, since two of them depend on budget approvals that could slip into January.
>
> I'd like to discuss the hiring plan and the pricing changes for the mid-market tier in more detail at the offsite next month. Please send me your comments on the draft forecast by Friday.

### `en-l02`

<sub>~3 min 08 s · engineering, software, incident, numbers, names</sub>

> This is the postmortem for the outage on Tuesday, October 7.
>
> Summary: For 47 minutes, between 2:13 and 3:00 pm Central European Time, about 35 percent of API requests failed with a 503 error. Customers using the mobile app could not log in, and webhooks to partners were delayed by up to two hours. No data was lost.
>
> Timeline: At 1:58 pm, we deployed version 4.12 of the authentication service. The release included a change to the connection pool settings for the Redis cache. At 2:13 pm, the error rate alarm fired, and the on-call engineer, Daniel, was paged. At first, he suspected a problem with the load balancer, because the health checks were flapping. At 2:31 pm, he escalated to the platform team. At 2:44 pm, we correlated the errors with the deployment and started a rollback. The rollback finished at 2:58 pm, and error rates returned to normal two minutes later.
>
> Root cause: The new configuration reduced the maximum number of Redis connections per pod from 50 to 5. This was a typo in a YAML file; the intended value was 500. Under normal traffic, five connections were enough, so the canary deployment looked healthy. When the afternoon traffic peak arrived, requests started queuing for a connection, the latency went above the 2 second timeout, and the pods were marked as unhealthy by Kubernetes. Because all pods were affected at the same time, the autoscaler could not compensate.
>
> What went well: The alert fired within one minute, the rollback procedure worked as documented, and the status page was updated within ten minutes.
>
> What went wrong: The canary phase only lasted 15 minutes and ran during low traffic, so it did not catch a capacity problem. Our dashboards did not show connection pool saturation, which made the diagnosis slower. Also, it took 31 minutes to connect the incident to the deployment, because the deploy notifications go to a different Slack channel than the alerts.
>
> Action items: First, add validation for connection pool settings in the configuration schema, with sane minimum and maximum values. Owner: Priya, due October 17. Second, extend the canary phase to at least one hour, or until it has handled a defined amount of traffic. Owner: platform team. Third, add a connection pool saturation panel to the service dashboard and an alert at 80 percent. Owner: Daniel. Fourth, post deployment events into the incident channel automatically, so that on-call engineers see them next to the alerts.
>
> We'll review the progress on these action items in the reliability sync in two weeks. Thanks to everyone who helped resolve this quickly.

### `mx-l01`

<sub>~2 min 48 s · DE / EN · sentence-switch, business, software, numbers, names</sub>

> *DE* Hallo zusammen, schön, dass ihr alle da seid.  
> *EN* Since we have colleagues from the Boston office joining today, I'll switch between German and English.  
> *DE* Wenn etwas unklar ist, fragt einfach nach.
>
> *EN* Let's start with the product launch.  
> *DE* Der Launch-Termin bleibt der 12. November, daran ändert sich nichts.  
> *EN* However, the marketing team needs the final feature list by the end of this week.  
> *DE* Lena, kannst du die Liste bis Donnerstag mit dem Engineering abstimmen?
>
> *EN* Next topic: the beta feedback.  
> *EN* We had about 340 beta users, and the overall satisfaction score was 4.3 out of 5.  
> *DE* Die häufigste Beschwerde war die Synchronisation zwischen Desktop und Smartphone.  
> *DE* Manche Nutzer mussten die App neu starten, damit ihre Notizen angezeigt wurden.  
> *EN* The engineering team has already identified the cause, it's a race condition in the offline cache.  
> *DE* Der Fix ist im aktuellen Sprint eingeplant.
>
> *DE* Jetzt zum Thema Pricing.  
> *EN* We're still debating between a single plan at 12 dollars per month and a tiered model with a free version.  
> *DE* Ich persönlich bin für das gestaffelte Modell, weil wir damit mehr Nutzer gewinnen.  
> *EN* On the other hand, a free tier increases our support costs and infrastructure spend.  
> *EN* Mark, could you run the numbers for both scenarios before our next meeting?
>
> *DE* Ein weiterer Punkt ist die Lokalisierung.  
> *DE* Für den Start planen wir Deutsch, Englisch und Französisch.  
> *EN* Spanish and Japanese will follow in the first quarter of next year.  
> *DE* Die Übersetzungen macht eine Agentur, aber die Fachbegriffe prüfen wir intern.
>
> *EN* Regarding hiring, we've opened two positions: a senior backend engineer and a product designer.  
> *DE* Die Stellenanzeigen sind seit Montag online, und wir haben schon über 60 Bewerbungen.  
> *EN* If you know anyone who might be a good fit, please send them my way.
>
> *EN* Last but not least, the budget.  
> *DE* Wir liegen aktuell bei 92 Prozent des geplanten Budgets für dieses Jahr.  
> *EN* That leaves us with roughly 80,000 euros for the launch campaign.  
> *DE* Das ist knapp, aber machbar, wenn wir auf die geplante Messe in Las Vegas verzichten.
>
> *DE* Okay, das war's von meiner Seite.  
> *EN* Any questions before we wrap up?  
> *DE* Wenn nicht, dann sehen wir uns nächste Woche zur gleichen Zeit.  
> *EN* Thanks, everyone.

### `fr-l01`

<sub>~2 min 43 s · business, engineering, manufacturing, numbers, names</sub>

> Voici le compte rendu de la réunion de pilotage du 9 octobre.
>
> Étaient présents : Amélie Chevalier, directrice des opérations, Benoît Girard, responsable du bureau d'études, Sophie Martin pour les achats, et moi-même.
>
> Premier point : l'avancement du projet de nouvelle ligne d'assemblage à l'usine de Clermont-Ferrand. L'installation des robots est terminée à 80 pour cent. Les essais de mise en service commenceront le 3 novembre, avec une semaine de retard par rapport au planning initial. Ce retard s'explique par la livraison tardive des convoyeurs, mais il ne devrait pas avoir d'impact sur le démarrage de la production en janvier.
>
> Deuxième point : la qualité. Le taux de rebut sur la ligne actuelle est passé de 2,1 à 3,4 pour cent au cours du dernier mois. Benoît a présenté une analyse des causes. Il s'agit principalement d'un problème d'usure sur un moule d'injection et d'un réglage de température mal adapté. Le moule sera remplacé la semaine prochaine, et une procédure de contrôle renforcée est déjà en place.
>
> Troisième point : les achats. Le prix de l'aluminium a augmenté de 12 pour cent depuis le début de l'année. Sophie propose de signer un contrat à prix fixe sur 18 mois avec notre fournisseur principal, afin de sécuriser nos coûts. Le comité est favorable à cette proposition, sous réserve d'une validation par la direction financière.
>
> Quatrième point : les ressources humaines. Nous devons recruter six techniciens de maintenance avant le démarrage de la nouvelle ligne. Trois postes sont déjà pourvus. Pour les autres, nous allons travailler avec une agence spécialisée et proposer des contrats en alternance avec le lycée technique de la région.
>
> Cinquième point : le budget. Les dépenses engagées s'élèvent à 4,6 millions d'euros sur un budget total de 5,2 millions. Il reste donc environ 600 000 euros pour la fin du projet, ce qui paraît suffisant si aucun imprévu majeur ne survient.
>
> Décisions prises : le remplacement du moule est validé, le contrat à prix fixe pour l'aluminium sera présenté au directeur financier, et la date de démarrage de la production est maintenue au 15 janvier.
>
> La prochaine réunion de pilotage aura lieu le 6 novembre à 10 heures, dans la salle de conférence du deuxième étage. Merci à tous pour votre participation.

## No speech (hallucination check)

3 clips, ~60 s in total.

### `noise-01` · Room silence

<sub>~20 s · silence</sub>

*20 seconds of normal room silence. Do not speak.*

### `noise-02` · Keyboard typing

<sub>~20 s · silence, noise</sub>

*20 seconds of typing on your keyboard, close to the mic. Do not speak.*

### `noise-03` · Background noise

<sub>~20 s · silence, noise</sub>

*20 seconds of a fan, street noise or café ambience without understandable speech. Do not speak.*

## Condition re-recordings

Record these scripts a second time under a different condition. Because the text is identical, the benchmark can compare clean and hard conditions directly on the same words. File name: `<id>@<condition>`.

| File name | Script | Condition |
|---|---|---|
| `de-s03@noisy` | `de-s03` | Play café noise, music without lyrics, or a podcast quietly in the background (speech at a lower level than your voice). |
| `de-s05@noisy` | `de-s05` | Play café noise, music without lyrics, or a podcast quietly in the background (speech at a lower level than your voice). |
| `en-s05@noisy` | `en-s05` | Play café noise, music without lyrics, or a podcast quietly in the background (speech at a lower level than your voice). |
| `mx-s05@noisy` | `mx-s05` | Play café noise, music without lyrics, or a podcast quietly in the background (speech at a lower level than your voice). |
| `de-s04@far` | `de-s04` | Sit about 2 metres away from the microphone. |
| `en-s04@far` | `en-s04` | Sit about 2 metres away from the microphone. |
| `de-s13@fast` | `de-s13` | Read noticeably faster than normal, as when you are in a hurry. |
| `en-s13@fast` | `en-s13` | Read noticeably faster than normal, as when you are in a hurry. |
| `de-s02@quiet` | `de-s02` | Speak softly, almost whispering, as in an open-plan office. |
| `en-s02@quiet` | `en-s02` | Speak softly, almost whispering, as in an open-plan office. |
| `de-s10@other-speaker` | `de-s10` | Ask another person to read it (different voice, accent or gender). |
| `en-s06@other-speaker` | `en-s06` | Ask another person to read it (different voice, accent or gender). |
| `mx-s03@other-speaker` | `mx-s03` | Ask another person to read it (different voice, accent or gender). |

## Checklist

- **German: short dictations:** `de-s01` `de-s02` `de-s03` `de-s04` `de-s05` `de-s06` `de-s07` `de-s08` `de-s09` `de-s10` `de-s11` `de-s12` `de-s13` `de-s14` `de-s15`
- **English: short dictations:** `en-s01` `en-s02` `en-s03` `en-s04` `en-s05` `en-s06` `en-s07` `en-s08` `en-s09` `en-s10` `en-s11` `en-s12` `en-s13` `en-s14` `en-s15`
- **Mixed language: short dictations:** `mx-s01` `mx-s02` `mx-s03` `mx-s04` `mx-s05` `mx-s06` `mx-s07` `mx-s08` `mx-s09` `mx-s10`
- **French: short dictations:** `fr-s01` `fr-s02` `fr-s03` `fr-s04` `fr-s05` `fr-s06`
- **Long recordings:** `de-l01` `de-l02` `en-l01` `en-l02` `mx-l01` `fr-l01`
- **No speech (hallucination check):** `noise-01` `noise-02` `noise-03`
- **Condition re-recordings:** `de-s03@noisy` `de-s05@noisy` `en-s05@noisy` `mx-s05@noisy` `de-s04@far` `en-s04@far` `de-s13@fast` `en-s13@fast` `de-s02@quiet` `en-s02@quiet` `de-s10@other-speaker` `en-s06@other-speaker` `mx-s03@other-speaker`
