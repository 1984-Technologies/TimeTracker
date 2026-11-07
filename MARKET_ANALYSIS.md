# Market analysis

| Product | What it already does | What it doesn't do |
| --- | --- | --- |
| [ActivTrak](https://www.activtrak.com/) | Detection of repetitive input and known simulation tools, apps/sites classification | Some features are Windows-only |
| [Insightful](https://www.insightful.io/) | Detection of repetitive input and known simulation tools, apps/sites classification | App/site labels too broad (Productive/Unproductive) |
| [Teramind](https://www.teramind.co/) | Screen OCR, behavior rules, recordings | Limited Linux support for website monitoring |
| [Hubstaff](https://hubstaff.com/) | Tracking of task time, idle time, inputs, screenshots and unusual-activity analytics | No automatic classification of the screen content |
| [Time Doctor](https://www.timedoctor.com/) | Tracking of task time, idle time, app/site use and screenshots | No automatic classification of the screen content |
| [Kloy](https://kloyapp.com/) | Screen content classification and matching with current project | Only MacOS is supported, no fraud detection |
| [ActivityWatch](https://activitywatch.net/) | Open source local app, browser and away-time tracking, regex-based rules | No fraud detection and tracking of the screen content |
| [Solidtime](https://www.solidtime.io/) | Open source self-hostable team time tracker |No fraud detection and tracking of the screen content |

# Unique selling proposition

Task-aware monitoring: local classification of the screen content and estimation of its relevance to the current task.

We are going to test [EmbeddingGemma 2](https://huggingface.co/google/embeddinggemma-2), as well as other small encoders that can be efficiently inferred on a CPU. 
The resulting embedding vectors will then be used for classification, clustering and other analyses
