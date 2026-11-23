# Course feedback survey (synthetic demo data)

Data for the UMSI AI Clinic Lab **Analyzing Data with Coding Agents** (Elle O'Brien, University of Michigan School of Information, November 10, 2026).

> **Not real data!** Everything here was generated for teaching. The XYZ department, its courses, the students, and their email addresses are all made up. Any resemblance to a real course is a coincidence.

## About the survey

At the end of the Spring 2026 semester, students in four courses in the XYZ department got an online course feedback survey. Responses were collected April 20–30, 2026 and exported from the survey platform as a CSV, one row per submitted response.

| Course | Name |
|---|---|
| XYZ 110 | Intro to Beekeeping |
| XYZ 120 | Pastry Science |
| XYZ 230 | Applied Knot Theory |
| XYZ 240 | Lighthouse Management |

## Files

| File | Contents |
|---|---|
| `data/survey.csv` | Survey export (219 rows, 8 columns) |
| `scripts/` | Empty, for your analysis code |

## Columns

| Column | Description |
|---|---|
| `response_id` | ID assigned by the survey platform to each submitted response |
| `submitted_at` | Date and time the response was submitted (`YYYY-MM-DD HH:MM`, Eastern time) |
| `email` | Respondent's school email address |
| `course` | Course code |
| `course_name` | Course name |
| `rating` | Answer to *"Overall, how would you rate this course?"* on a 1–5 scale (1 = Poor, 5 = Excellent). `N/A` means the respondent skipped the question. |
| `would_recommend` | Answer to *"Would you recommend this course to another student?"* (`Yes` / `No`) |
| `comments` | Optional free-text answer to *"Anything else you'd like to tell us?"* Often blank. |

## Trying it with a coding agent

Open this folder in a coding agent (Claude Code, Codex, GitHub Copilot, …) and ask it a question about the data. For example: *"What's the average rating for each course? Save the code as a script."*

Then look at what it did, not just what it said. What did it decide that you didn't ask for?

## License

The data and this README are released under [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/): use them for anything, no attribution needed.
