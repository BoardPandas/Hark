# UX Laws and Best Practices Reference

Source: https://lawsofux.com/ (Jon Yablonski)

This reference is loaded by the UX review skill. Each law includes the principle, key takeaways, and application guidance for evaluating UI/UX code.

## Decision Making

### 1. Hick's Law

**Principle:** Decision time increases with the number and complexity of choices.

- Minimize choices when response speed matters
- Break complex tasks into manageable steps
- Highlight recommended options to prevent overwhelm
- Introduce features gradually during onboarding
- Do not oversimplify to the point of hiding necessary depth
- Origin: Hick & Hyman, 1952

**What to look for in code:** Navigation menus with excessive items, forms presenting all fields at once, settings pages without grouping, onboarding flows that front-load all features.

### 2. Choice Overload (Paradox of Choice)

**Principle:** Users become overwhelmed when presented with too many options.

- Excessive choices degrade decision quality and satisfaction
- Provide comparison tools when options are necessary (e.g., pricing tiers)
- Use progressive disclosure, intelligent defaults, and filtering
- Origin: Alvin Toffler, *Future Shock* (1970)

**What to look for in code:** Dropdown menus with 20+ items without search, filter panels with too many simultaneous options, product grids without sorting/filtering, action menus without grouping.

### 3. Occam's Razor

**Principle:** Among competing solutions of equal effectiveness, choose the simplest.

- Prevent complexity rather than removing it later
- Evaluate every element; remove what doesn't serve an essential function
- Design is complete when further removal would compromise usability
- Origin: William of Ockham, c. 1287-1347

**What to look for in code:** Redundant UI elements, decorative elements that add no clarity, multi-step flows that could be single-step, unnecessary confirmation dialogs.

### 4. Pareto Principle (80/20 Rule)

**Principle:** Roughly 80% of effects come from 20% of causes.

- A small subset of features/flows drives the majority of value
- Focus resources on high-impact areas
- Use data to identify the "vital few" that drive satisfaction
- Origin: Vilfredo Pareto, Italian economist

**What to look for in code:** Equal visual weight given to all features (primary and tertiary), no clear visual hierarchy between frequent and rare actions, settings pages treating every option as equally important.

---

## Memory and Attention

### 5. Miller's Law

**Principle:** Working memory holds about 7 (plus or minus 2) items.

- Do not use 7 as a rigid cap; use it as a signal to organize better
- Break content into meaningful chunks to enhance processing
- Memory capacity varies by background knowledge and context
- Origin: George A. Miller, 1956

**What to look for in code:** Navigation with 10+ top-level items, long unstructured lists, forms with many ungrouped fields, dashboards with too many data points competing for attention.

### 6. Serial Position Effect

**Principle:** Users best remember the first and last items in a sequence.

- Place critical actions at the beginning and end of lists/nav bars
- Less important items can live in the middle
- Combines primacy effect (first items) and recency effect (last items)
- Origin: Herman Ebbinghaus

**What to look for in code:** Important CTAs buried in the middle of navigation, key information placed mid-list, tab bars where critical items are not at the edges.

### 7. Von Restorff Effect (Isolation Effect)

**Principle:** The visually distinctive item among similar objects is the most memorable.

- Make critical information and primary CTAs visually stand out
- Use emphasis selectively; too many highlighted elements compete and look like ads
- Do not rely solely on color for contrast (accessibility)
- Be cautious with motion-based contrast (motion sensitivity)
- Origin: Hedwig von Restorff, 1933

**What to look for in code:** Primary and secondary buttons with identical styling, no visual distinction for the recommended pricing tier, multiple elements competing for attention with bold/color, color-only emphasis without alternative cues.

### 8. Zeigarnik Effect

**Principle:** People remember uncompleted tasks better than completed ones.

- Use clear indicators to signal additional content and encourage exploration
- Progress bars with artificial head starts increase completion motivation
- Transparent progress tracking sustains engagement
- Origin: Bluma Zeigarnik, 1920s

**What to look for in code:** Multi-step forms without progress indicators, onboarding flows without completion tracking, profile setup without visible completion percentage.

### 9. Selective Attention

**Principle:** Users focus on goal-relevant stimuli and filter out the rest.

- Users have tunnel vision toward their current objective
- Important information outside their task focus may go unseen
- Design for the user's current task, not for everything simultaneously

**What to look for in code:** Critical warnings placed far from the user's focus area, important status changes communicated only via subtle color changes, banner blindness patterns.

### 10. Working Memory

**Principle:** Temporary cognitive system that holds information needed for the current task.

- Capacity is limited (see Miller's Law)
- Reduce demands through chunking and external cues
- Do not require users to remember information from previous screens

**What to look for in code:** Flows requiring users to memorize values from a previous step, reference codes displayed once then needed later, cart/checkout flows where key details disappear.

---

## Perception and Grouping (Gestalt Principles)

### 11. Law of Proximity

**Principle:** Objects near each other are perceived as belonging together.

- Spatial closeness creates functional associations
- Strategic spacing improves scanning and comprehension
- Origin: Gestalt psychology

**What to look for in code:** Form labels far from their inputs, related buttons with too much space between them, unrelated elements packed too closely, inconsistent spacing between groups.

### 12. Law of Similarity

**Principle:** Visually similar elements are perceived as related or grouped.

- Color, shape, size, orientation, and movement signal group membership
- Links and interactive elements must be visually distinct from regular text
- Origin: Gestalt psychology

**What to look for in code:** Clickable and non-clickable elements styled identically, inconsistent button styles for the same action type, status indicators using arbitrary colors without pattern.

### 13. Law of Common Region

**Principle:** Elements within a shared boundary are perceived as related.

- Use borders or background colors to create visual grouping
- Establishes hierarchy and helps users grasp relationships quickly
- Origin: Gestalt psychology

**What to look for in code:** Related form sections without visual containers, card layouts where content bleeds across boundaries, dashboard widgets without clear separation.

### 14. Law of Uniform Connectedness

**Principle:** Visually connected elements are perceived as more related than unconnected ones.

- Use colors, lines, frames, or shapes to connect related elements
- Use tangible connectors (lines, arrows) to reinforce relationships
- Origin: Gestalt psychology

**What to look for in code:** Stepper/wizard flows without connecting lines, related data displayed without visual links, timeline components without connecting elements.

### 15. Law of Pragnanz (Simplicity)

**Principle:** People interpret complex visuals in the simplest possible form.

- Simple figures are processed and remembered more easily
- The brain automatically "simplifies" complex shapes
- Origin: Max Wertheimer, 1910

**What to look for in code:** Overly complex icons or illustrations, charts with too many data series, layouts with competing visual patterns, unnecessary visual complexity.

### 16. Chunking

**Principle:** Break information into meaningful, coherent groups.

- Improves scannability and processing speed
- Reveals relationships between related content
- Origin: George A. Miller, 1956

**What to look for in code:** Long unbroken text blocks, phone/card number inputs without formatting, data tables without row grouping, settings pages as flat lists.

---

## Interaction and Performance

### 17. Fitts's Law

**Principle:** Time to acquire a target depends on distance to and size of the target.

- Touch targets must be large enough for accurate selection (minimum 44x44px on mobile)
- Adequate spacing between interactive elements prevents accidental clicks
- Place frequently used controls within easy reach
- Rapid movements toward small targets increase error rates
- Origin: Paul Fitts, 1954

**What to look for in code:** Small click/tap targets (under 44px on mobile, under 24px on desktop), interactive elements too close together, important actions far from the user's cursor focus area, tiny icon-only buttons without adequate hit areas.

### 18. Doherty Threshold

**Principle:** Productivity soars when system response time is under 400ms.

- System feedback must arrive within 400ms to maintain attention
- Perceived performance matters as much as actual speed
- Use animations and progress bars during unavoidable delays
- Origin: Doherty & Thadani, 1982, IBM Systems Journal

**What to look for in code:** API calls without loading states, form submissions without feedback, page transitions without skeleton screens, long operations without progress indication, missing optimistic UI updates.

### 19. Parkinson's Law

**Principle:** Tasks expand to fill available time.

- Set and communicate realistic time expectations
- Use autofill, pre-populated fields, and streamlined flows
- Constraints can improve the experience
- Origin: Cyril Northcote Parkinson, 1955

**What to look for in code:** Forms with unnecessary optional fields, checkout flows with excessive steps, registration requiring information not needed immediately.

### 20. Flow

**Principle:** A state of complete immersion and focus during an activity.

- Minimize interruptions that break flow state
- Remove unnecessary friction in task completion paths

**What to look for in code:** Modal dialogs interrupting task completion, forced redirects mid-flow, unnecessary confirmation prompts, auto-playing media disrupting focus.

---

## User Behavior and Expectations

### 21. Jakob's Law

**Principle:** Users expect your site to work like others they already know.

- Users transfer mental models from familiar products
- Leverage established design patterns
- When redesigning, allow temporary access to the old version
- Origin: Jakob Nielsen, Nielsen Norman Group

**What to look for in code:** Non-standard navigation patterns, custom controls replacing native HTML elements without matching behavior, unconventional icon usage, search bars in unexpected locations, hamburger menus on desktop.

### 22. Postel's Law (Robustness Principle)

**Principle:** Be liberal in what you accept, be conservative in what you send.

- Accept variable user input gracefully; interpret intent charitably
- System outputs must remain strict, predictable, and reliable
- Origin: Jon Postel, TCP specification

**What to look for in code:** Strict input validation rejecting valid formats (e.g., phone numbers with spaces), fields requiring exact format without guidance, error messages that don't explain what format is expected, forms that clear on validation failure.

### 23. Mental Model

**Principle:** Users carry compressed internal representations of how systems work.

- Users see their idea of the system, not the actual system
- Mismatched mental models cause confusion and errors

**What to look for in code:** Terminology inconsistent with industry norms, UI metaphors that don't match user expectations, navigation structures that contradict the conceptual model.

### 24. Paradox of the Active User

**Principle:** Users start using software immediately without reading instructions.

- Users learn by doing, not by reading
- Design for exploration and recoverability
- Provide inline guidance and contextual help

**What to look for in code:** Critical instructions only in documentation/help pages, features requiring upfront reading before use, no inline tooltips or contextual help, destructive actions without undo.

### 25. Cognitive Bias

**Principle:** Systematic thinking errors influence perception and decision-making.

- Users don't behave rationally
- Negativity bias makes bad experiences more memorable
- Design for real human behavior, not ideal behavior

**What to look for in code:** Assumptions that users will read all content, design relying on rational comparison of all options, error states without recovery paths.

---

## Experience Design

### 26. Aesthetic-Usability Effect

**Principle:** Users perceive aesthetically pleasing design as more usable.

- Attractive interfaces trigger positive responses that cause users to overestimate functionality
- Users show greater patience with minor issues when visual design is polished
- Beautiful interfaces can mask usability problems during testing
- Origin: Kurosu & Kashimura, 1995, Hitachi Design Center

**What to look for in code:** Inconsistent visual polish across the app, placeholder/unstyled states in production, jarring style differences between sections, broken layouts at common viewport sizes.

### 27. Peak-End Rule

**Principle:** Experiences are judged by their most intense moment and their ending, not the average.

- Design specifically for peak emotional moments in user journeys
- Endings disproportionately influence overall perception
- Negative peaks register more strongly than positive ones
- Origin: Kahneman et al., 1993

**What to look for in code:** Bland success/completion states, error pages with no personality or recovery options, abrupt session endings, checkout completion without celebration or clear next steps.

### 28. Goal-Gradient Effect

**Principle:** Motivation increases as users approach completion of a goal.

- Users work faster as they near the finish line
- Artificial progress indicators boost completion rates
- Origin: Clark Hull, 1932

**What to look for in code:** Multi-step processes without progress visualization, onboarding flows without completion indicators, loyalty/reward systems without milestone tracking.

### 29. Tesler's Law (Conservation of Complexity)

**Principle:** Every system has irreducible complexity that can only be shifted, not eliminated.

- Complexity must live either in the system or with the user; choose the system
- One engineer's extra week beats millions of users losing minutes daily
- Embed contextual help for unavoidable complexity
- Origin: Larry Tesler, Xerox PARC, mid-1980s

**What to look for in code:** Complex configuration dumped on users without defaults, technical jargon in user-facing text, raw error codes without human-readable messages, requiring users to understand system internals.

### 30. Cognitive Load

**Principle:** The total mental resources required to understand and interact with an interface.

- Every element on screen adds to cognitive burden
- Reduce through chunking, progressive disclosure, and removing unnecessary elements
- Three types: intrinsic (task complexity), extraneous (poor design), germane (learning)

**What to look for in code:** Dense layouts with no visual breathing room, multiple competing calls-to-action, information overload on landing pages, complex forms without logical sections.
