## Conversation Evidence

> user: "i see that on the desktop top you have a timeline could you prototype one for the phone view?"
> user: "dont do two lines for the timeline"
> user: "Also would it be possible to have a seperate color for on site/client visits"
> user: "could you remove the ligher green from the timeline in the work period" / lunch "could get its own segment instead of drawing ontop"
> user (Trips): "Keep it" · (Day numbers): "match the mockups but add a milage in some way" · (Komme/gå, Kørsel): "Keep Danish"
> user: "also make the lunch line in the point form overview editable with the text "edit" like private"

## Goal & Context

Build the approved phone Day view design (mockups at https://claude.ai/artifact/K5zwEk1uPeNuifnnaZjZ2W, Hours · Day row: "Phone · Hours › Day · add or edit…", "… · today", "… · edit lunch · sheet open") into the app, after 0.4.0 (merged on main).

## Acceptance Criteria

- **R1:** Phone Day view has a one-line timeline 06–18 above the day's list: planned day as a dashed outline; work green (office and home the same green); client visits violet; private time blue; lunch its own solid grey segment between work blocks; driving light blue on the same line; today shows a thin "now" line. Legend: Planned · Work · Client · Private · Lunch · Driving.
- **R2:** Client visits use violet everywhere: timeline (phone and desktop Day), the Hours day-list chip for client days, and the Mileage "Client visit" rule chip (light #5b3f8c on #efe9f8, bar #7a5bb5; dark #c3aef0 on #2a2238, bar #9d82d6).
- **R3:** Day numbers: three numbers Worked · Norm · Diff (today: Worked so far · Expected so far · Diff with their small lines kept); no other small text; no Mileage card.
- **R4:** Trips section under the day's list: time, From → To, km, type chip (Commute grey, Client visit violet), and "+ Add missing trip" (the existing manual-trip form); its heading shows the day's mileage: "Trips · 37,0 km · 0,00 kr".
- **R5:** The lunch line in the day's list has "Edit" opening a lunch sheet (Start, Minutes, Use default, Save) through the existing lunch override; the separate Lunch break card is removed.
- **R6:** Desktop Day timeline also on one line (planned outline, segments, driving inline) with the client colour.

## Boundaries

- Danløn field names "Komme/gå" and "Kørsel" stay Danish.
- The APK is built and signed only by the GitHub release workflow from a version tag.
